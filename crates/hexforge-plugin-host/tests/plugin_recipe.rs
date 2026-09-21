//! Recipe↔plugin contract on the REAL template component.
//!
//! Covers: canonical `plugin:<id>` operation ids (builtin ids can never be
//! shadowed — the mapping is structural), `origin() == "plugin"`, install-time
//! denial of ungranted privileged capabilities, and — the security crux —
//! that `revoke` blocks an ALREADY-REGISTERED instance: execute re-reads
//! effective grants from the on-disk `grants.json` instead of trusting the
//! in-memory manifest snapshot.

use hexforge_plugin_host::store::{PluginLibrary, PluginStatus};
use hexforge_plugin_host::{
    canonical_op_id, generate_keypair, sign_manifest, PluginRuntime, PLUGIN_OP_PREFIX,
};

const WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");

fn fresh_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hexforge-recipe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Template manifest with `network` requested; `granted` toggles the case.
fn network_manifest_bytes(granted: bool) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    v["requested_capabilities"] = serde_json::json!(["network"]);
    v["granted_capabilities"] = if granted {
        serde_json::json!(["network"])
    } else {
        serde_json::json!([])
    };
    serde_json::to_vec(&v).unwrap()
}

fn install(
    root: &std::path::Path,
    manifest: &[u8],
) -> (PluginLibrary, hexforge_plugin_host::PluginInstance) {
    let (pubkey, secret) = generate_keypair();
    let sig = sign_manifest(manifest, &secret).unwrap();
    let lib = PluginLibrary::new(root.to_path_buf(), Vec::new());
    let found = lib
        .install_package(WASM, manifest, &sig, &pubkey)
        .expect("template component installs");
    assert_eq!(found.status, PluginStatus::Verified);
    let instance = found.instance.expect("verified carries instance");
    (lib, instance)
}

#[test]
fn op_id_is_canonical_prefixed_and_origin_is_plugin() {
    let root = fresh_root();
    let (_lib, instance) = install(&root, MANIFEST.as_bytes());
    let runtime = std::sync::Arc::new(PluginRuntime::new(None).unwrap());
    let transform = runtime.as_transform(instance).unwrap();
    assert_eq!(
        hexforge_core::Transform::id(&transform),
        "plugin:example.wit-uppercase"
    );
    assert_eq!(hexforge_core::Transform::origin(&transform), "plugin");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn canonical_op_id_never_collides_with_builtins() {
    assert_eq!(PLUGIN_OP_PREFIX, "plugin:");
    assert_eq!(
        canonical_op_id("example.wit-uppercase"),
        "plugin:example.wit-uppercase"
    );
    // A manifest id that spells a builtin op still lands in the plugin
    // namespace — shadowing is impossible by construction.
    assert_eq!(
        canonical_op_id("encoding.base64.encode"),
        "plugin:encoding.base64.encode"
    );
}

#[test]
fn install_denies_ungranted_privileged_capability() {
    let root = fresh_root();
    let manifest = network_manifest_bytes(false);
    let (pubkey, secret) = generate_keypair();
    let sig = sign_manifest(&manifest, &secret).unwrap();
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let err = lib
        .install_package(WASM, &manifest, &sig, &pubkey)
        .unwrap_err();
    assert!(err.to_string().contains("network"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn revoke_blocks_already_registered_instance() {
    let root = fresh_root();
    // Install WITH the grant: the returned (registered) instance snapshot
    // carries `network` as granted.
    let (lib, instance) = install(&root, &network_manifest_bytes(true));
    let runtime = PluginRuntime::new(None).unwrap();
    let out = runtime.execute(&instance, b"hello").expect("granted runs");
    assert_eq!(out, b"HELLO");

    // Revoke touches only `grants.json` — the stale in-memory snapshot still
    // claims the grant. Execute must deny anyway (live re-read).
    lib.set_grants("example.wit-uppercase", &[]).unwrap();
    let err = runtime.execute(&instance, b"hello").unwrap_err();
    assert!(err.to_string().contains("network"), "{err}");

    // A freshly discovered instance agrees (single source of truth).
    let fresh = PluginLibrary::new(root.clone(), Vec::new())
        .get("example.wit-uppercase")
        .unwrap()
        .instance
        .unwrap();
    let err = runtime.execute(&fresh, b"hello").unwrap_err();
    assert!(err.to_string().contains("network"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn corrupted_grants_file_fails_closed_at_execute() {
    let root = fresh_root();
    let (_lib, instance) = install(&root, &network_manifest_bytes(true));
    std::fs::write(
        root.join("example.wit-uppercase").join("grants.json"),
        b"{corrupted",
    )
    .unwrap();
    let runtime = PluginRuntime::new(None).unwrap();
    let err = runtime.execute(&instance, b"hello").unwrap_err();
    assert!(!err.to_string().is_empty(), "must fail closed");
    let _ = std::fs::remove_dir_all(&root);
}
