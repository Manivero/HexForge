//! Plugin SDK lifecycle: keygen → manifest → sign → install → grant → execute.
//!
//! CI-friendly: builds the example plugin WASM from `plugins/example-uppercase`
//! WAT at test time (no prebuilt binary, no network, temp dirs only).
//! Legacy core-module plugins are covered on purpose: the host must keep
//! executing them through the manifest fallback (backward compatibility).

use hexforge_core::transform::NullExecutionContext;
use hexforge_core::Transform;
use hexforge_plugin_host::{
    generate_keypair, sign_manifest, validate_manifest, verify_signature, PluginManifest,
    PluginRuntime, WIT_PACKAGE, WIT_VERSION,
};
use std::borrow::Cow;
use std::sync::Arc;

const EXAMPLE_WAT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/example-uppercase/plugin.wat"
);
const HOST_WIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wit/plugin.wit");
const TEMPLATE_WIT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/example-wit/wit/plugin.wit"
);

fn example_manifest() -> PluginManifest {
    PluginManifest {
        id: "example.uppercase".into(),
        name: "Example Uppercase".into(),
        version: "1.0.0".into(),
        author: "HexForge Example".into(),
        requested_capabilities: vec![],
        granted_capabilities: vec![],
    }
}

fn build_example_wasm(tag: &str) -> String {
    let wat = std::fs::read_to_string(EXAMPLE_WAT).expect("example plugin.wat must exist");
    let bytes = wat::parse_str(&wat).expect("example WAT must assemble");
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hexforge-lifecycle-{tag}-{}.wasm",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, &bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn install_signed(
    runtime: &PluginRuntime,
    wasm_path: &str,
    manifest: &PluginManifest,
) -> (hexforge_plugin_host::PluginInstance, String) {
    let manifest_bytes = serde_json::to_vec(manifest).unwrap();
    validate_manifest(manifest).expect("test manifest must validate");
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).unwrap();
    assert!(verify_signature(&manifest_bytes, &sig_hex, &pubkey_hex).unwrap());
    let instance = runtime
        .install(
            std::path::Path::new(wasm_path),
            &manifest_bytes,
            &sig_hex,
            &pubkey_hex,
        )
        .expect("signed example plugin must install");
    (instance, wasm_path.to_string())
}

#[test]
fn lifecycle_install_grant_execute() {
    let wasm_path = build_example_wasm("happy");
    let runtime = Arc::new(PluginRuntime::new(None).unwrap());
    let manifest = example_manifest();
    let (instance, _) = install_signed(&runtime, &wasm_path, &manifest);

    // Legacy core module: metadata comes from the manifest fallback.
    let transform = runtime.clone().as_transform(instance).unwrap();
    assert_eq!(transform.id(), "example.uppercase");
    assert_eq!(transform.version(), "1.0.0");

    let out = transform
        .apply(
            Cow::Borrowed(b"hello world"),
            &serde_json::json!({}),
            &NullExecutionContext,
        )
        .unwrap();
    assert_eq!(out.as_ref(), b"HELLO WORLD");
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn lifecycle_capability_grant_and_revoke() {
    let wasm_path = build_example_wasm("caps");
    let runtime = PluginRuntime::new(None).unwrap();

    // Privileged cap requested but not granted → install refuses with guidance.
    let mut manifest = example_manifest();
    manifest.requested_capabilities = vec!["network".into()];
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).unwrap();
    let err = runtime
        .install(
            std::path::Path::new(&wasm_path),
            &manifest_bytes,
            &sig_hex,
            &pubkey_hex,
        )
        .unwrap_err();
    assert!(err.to_string().contains("capability denied"), "{err}");

    // Grant via manifest (UI `grant_capability` flow) → install succeeds…
    manifest.granted_capabilities = vec!["network".into()];
    let (mut instance, _) = install_signed(&runtime, &wasm_path, &manifest);

    // …granting an unrequested cap is itself an error (typo guard)…
    let err = PluginRuntime::grant_capability(&mut instance, "filesystem_read").unwrap_err();
    assert!(err.to_string().contains("not requested"), "{err}");

    // …and revoking the grant makes execution refuse (checked at execute too).
    PluginRuntime::revoke_capability(&mut instance, "network");
    let transform = Arc::new(runtime).as_transform(instance).unwrap();
    let err = transform
        .apply(
            Cow::Borrowed(b"hello"),
            &serde_json::json!({}),
            &NullExecutionContext,
        )
        .unwrap_err();
    assert!(err.to_string().contains("capability"), "{err}");
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn lifecycle_rejects_tampered_and_invalid() {
    let wasm_path = build_example_wasm("neg");
    let runtime = PluginRuntime::new(None).unwrap();
    let manifest = example_manifest();
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).unwrap();

    // Tampered bytes → signature error, plugin never loads.
    let mut tampered = manifest_bytes.clone();
    tampered[10] ^= 0xFF;
    let err = runtime
        .install(
            std::path::Path::new(&wasm_path),
            &tampered,
            &sig_hex,
            &pubkey_hex,
        )
        .unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");

    // Valid signature but bad semantics → developer-friendly manifest error.
    let mut bad = example_manifest();
    bad.version = "1.0".into();
    let bad_bytes = serde_json::to_vec(&bad).unwrap();
    let bad_sig = sign_manifest(&bad_bytes, &signing_key_hex).unwrap();
    let err = runtime
        .install(
            std::path::Path::new(&wasm_path),
            &bad_bytes,
            &bad_sig,
            &pubkey_hex,
        )
        .unwrap_err();
    assert!(err.to_string().contains("invalid manifest"), "{err}");

    // Valid signature + manifest but garbage binary → explicit load error.
    let mut garbage_path = std::env::temp_dir();
    garbage_path.push(format!(
        "hexforge-lifecycle-garbage-{}.wasm",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&garbage_path, b"definitely not wasm").unwrap();
    let err = runtime
        .install(&garbage_path, &manifest_bytes, &sig_hex, &pubkey_hex)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("neither a valid component nor a valid core module"),
        "{err}"
    );
    let _ = std::fs::remove_file(&wasm_path);
    let _ = std::fs::remove_file(&garbage_path);
}

#[test]
fn wit_contract_identity_stays_in_sync() {
    // The three copies of the contract identity must agree:
    // host consts, host WIT file, SDK template WIT file.
    assert_eq!(WIT_PACKAGE, "hexforge:plugin");
    assert_eq!(WIT_VERSION, "0.1.0");
    let needle = format!("package {WIT_PACKAGE}@{WIT_VERSION};");
    for path in [HOST_WIT, TEMPLATE_WIT] {
        let wit = std::fs::read_to_string(path).expect("WIT file must exist");
        assert!(
            wit.contains(&needle),
            "{path} must declare `{needle}` (versioning/ABI compatibility)"
        );
    }
}
