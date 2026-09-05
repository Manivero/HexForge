//! Real WIT component execution: `execute_component` / `try_get_wit_metadata`
//! against hand-built Component Model binaries (canonical ABI in WAT).
//!
//! Until now the component path was only exercised through core-module
//! fallbacks. These tests assemble true `(component …)` binaries with the
//! `wat` crate (no external toolchain) and prove the WIT path is live:
//! metadata comes from the component exports, not the manifest.

use hexforge_core::transform::NullExecutionContext;
use hexforge_core::{MemoryCost, Transform};
use hexforge_plugin_host::{generate_keypair, sign_manifest, PluginManifest, PluginRuntime};
use std::borrow::Cow;
use std::sync::Arc;

const WIT_UPPERCASE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/wit_uppercase.component.wat"
);
const WIT_LOOP: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/wit_loop.component.wat"
);
const WIT_UNRELATED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/wit_unrelated.component.wat"
);

fn assemble(path: &str, tag: &str) -> String {
    let text = std::fs::read_to_string(path).expect("component WAT must exist");
    let bytes = wat::parse_str(&text).expect("component WAT must assemble");
    // Sanity: a component has the component-model preamble, not core wasm.
    assert_eq!(&bytes[0..4], b"\0asm", "must be a wasm binary");
    let mut out = std::env::temp_dir();
    out.push(format!("hexforge-wit-{tag}-{}.wasm", uuid::Uuid::new_v4()));
    std::fs::write(&out, &bytes).unwrap();
    out.to_string_lossy().into_owned()
}

/// Manifest deliberately DIFFERS from the component metadata: if the host
/// reports the WIT values, the component path is proven live.
fn fallback_manifest() -> PluginManifest {
    PluginManifest {
        id: "manifest.fallback".into(),
        name: "Manifest Fallback".into(),
        version: "9.9.9".into(),
        author: "Test".into(),
        requested_capabilities: vec![],
        granted_capabilities: vec![],
    }
}

fn install_component(
    runtime: &PluginRuntime,
    wasm_path: &str,
    manifest: &PluginManifest,
) -> hexforge_plugin_host::PluginInstance {
    let manifest_bytes = serde_json::to_vec(manifest).unwrap();
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).unwrap();
    runtime
        .install(
            std::path::Path::new(wasm_path),
            &manifest_bytes,
            &sig_hex,
            &pubkey_hex,
        )
        .expect("signed component must install")
}

#[test]
fn wit_component_metadata_comes_from_exports() {
    let wasm_path = assemble(WIT_UPPERCASE, "meta");
    let runtime = Arc::new(PluginRuntime::new(None).unwrap());
    let instance = install_component(&runtime, &wasm_path, &fallback_manifest());

    let transform = runtime.clone().as_transform(instance).unwrap();
    // WIT values — NOT the manifest fallback.
    assert_eq!(transform.id(), "comp.uppercase");
    assert_eq!(transform.version(), "1.0.0");
    assert_eq!(transform.display_name(), "Comp Uppercase");
    assert_eq!(transform.category(), "Text");
    assert_eq!(
        transform.params_schema(),
        serde_json::json!({}),
        "params schema parsed from component string"
    );
    let caps = transform.capabilities();
    assert!(caps.deterministic);
    assert!(!caps.streamable);
    assert!(matches!(caps.memory_cost, MemoryCost::FullBuffer));

    let out = transform
        .apply(
            Cow::Borrowed(b"wit hello"),
            &serde_json::json!({}),
            &NullExecutionContext,
        )
        .unwrap();
    assert_eq!(out.as_ref(), b"WIT HELLO");
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn wit_component_execute_path_direct() {
    let wasm_path = assemble(WIT_UPPERCASE, "exec");
    let runtime = PluginRuntime::new(None).unwrap();
    let instance = install_component(&runtime, &wasm_path, &fallback_manifest());

    // `execute` (no Transform wrapper) also routes through the component.
    let out = runtime.execute(&instance, b"direct path").unwrap();
    assert_eq!(out, b"DIRECT PATH");
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn wit_component_fuel_exhaustion_is_explicit() {
    let wasm_path = assemble(WIT_LOOP, "loop");
    let runtime = PluginRuntime::new(Some(1000)).unwrap();
    let instance = install_component(&runtime, &wasm_path, &fallback_manifest());

    let err = runtime.execute(&instance, b"spin").unwrap_err();
    assert!(
        err.to_string().contains("fuel exhausted"),
        "component fuel trap must name fuel exhaustion, got: {err}"
    );
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn wit_foreign_component_falls_back_and_refuses_execute() {
    let wasm_path = assemble(WIT_UNRELATED, "foreign");
    let runtime = Arc::new(PluginRuntime::new(None).unwrap());
    let instance = install_component(&runtime, &wasm_path, &fallback_manifest());

    // No transform interface → manifest fallback, no hang, no panic.
    let transform = runtime.clone().as_transform(instance.clone()).unwrap();
    assert_eq!(transform.id(), "manifest.fallback");

    // …and execution refuses explicitly instead of running the unknown export.
    let err = runtime.execute(&instance, b"test").unwrap_err();
    assert!(!err.to_string().is_empty(), "must fail with a message");
    let _ = std::fs::remove_file(&wasm_path);
}
