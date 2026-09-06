//! SDK lifecycle on the REAL template-built component (`plugins/example-wit`).
//!
//! Unlike the WAT fixtures, `plugin.wasm` here is produced by the documented
//! developer flow (`cargo build --target wasm32-wasip1`, no_std and WASI-free,
//! then `wasm-tools component new`). The manifest is committed BOUND
//! (`wasm_sha256`) but UNSIGNED: each test signs ephemerally, so no secrets
//! live in the repo and CI needs no wasm toolchain.
//!
//! Covers: artifact/manifest sync, install → discovery → re-verification →
//! execution through the real component ABI, and the failure modes a
//! developer must see (bad signature, artifact mismatch, garbage binary,
//! capability violation, malformed manifest).

use hexforge_plugin_host::store::{PluginLibrary, PluginStatus};
use hexforge_plugin_host::{generate_keypair, sign_manifest, PluginManifest, PluginRuntime};

const WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn fresh_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hexforge-sdk-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn signed_package() -> (Vec<u8>, Vec<u8>, String, String) {
    let (pubkey, secret) = generate_keypair();
    let sig = sign_manifest(MANIFEST.as_bytes(), &secret).unwrap();
    (WASM.to_vec(), MANIFEST.as_bytes().to_vec(), sig, pubkey)
}

#[test]
fn sdk_template_artifact_matches_bound_manifest() {
    // Guard against shipping a stale .wasm next to a bound manifest.
    let manifest: PluginManifest = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest.id, "example.wit-uppercase");
    let bound = manifest
        .wasm_sha256
        .expect("template manifest must be bound via `plugin bind`");
    assert_eq!(
        bound,
        sha256_hex(WASM),
        "re-run `plugin bind` after rebuild"
    );
}

#[test]
fn sdk_component_install_discover_execute() {
    let (wasm, manifest, sig, pubkey) = signed_package();
    let root = fresh_root();

    // Install through the real backend path (same as Tauri `install_plugin`).
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let found = lib
        .install_package(&wasm, &manifest, &sig, &pubkey)
        .expect("template component installs");
    assert_eq!(found.status, PluginStatus::Verified);
    let stored = found.manifest.expect("verified carries manifest");
    assert_eq!(stored.id, "example.wit-uppercase");
    assert_eq!(stored.version, "1.0.0");
    let instance = found.instance.expect("verified carries instance");

    // Restart: fresh library on the same root re-verifies from disk.
    drop(lib);
    let lib2 = PluginLibrary::new(root.clone(), Vec::new());
    let instances = lib2.verified_instances();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].manifest.id, "example.wit-uppercase");

    // Execution goes through the component `apply`, not the echo fallback:
    // input "hello" must come back uppercased.
    let runtime = PluginRuntime::new(None).unwrap();
    let out = runtime.execute(&instance, b"hello").expect("apply runs");
    assert_eq!(out, b"HELLO");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sdk_install_rejects_developer_errors() {
    let (wasm, manifest, sig, pubkey) = signed_package();
    let root = fresh_root();
    let lib = PluginLibrary::new(root.clone(), Vec::new());

    // Foreign key: signature over the same bytes does not verify.
    let (other_pub, _) = generate_keypair();
    let err = lib
        .install_package(&wasm, &manifest, &sig, &other_pub)
        .unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");

    // Post-sign .wasm swap: binding mismatch fails closed.
    let mut swapped = wasm.clone();
    let last = swapped.len() - 1;
    swapped[last] ^= 0x01;
    let err = lib
        .install_package(&swapped, &manifest, &sig, &pubkey)
        .unwrap_err();
    assert!(
        err.to_string().contains("mismatch") || err.to_string().contains("sha256"),
        "{err}"
    );

    // Post-sign manifest edit: signature covers exactly the shipped bytes.
    let mut manifest_tampered = manifest.clone();
    manifest_tampered[20] ^= 0x01;
    let err = lib
        .install_package(&wasm, &manifest_tampered, &sig, &pubkey)
        .unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");

    // Garbage binary: not installable, nothing registered.
    let err = lib
        .install_package(b"not a wasm module", &manifest, &sig, &pubkey)
        .unwrap_err();
    assert!(lib.verified_instances().is_empty(), "{err}");

    // Privileged capability requested but not granted: install refuses.
    let net_manifest = MANIFEST.replace(
        "\"requested_capabilities\": []",
        "\"requested_capabilities\": [\"network\"]",
    );
    let (net_pub, net_sec) = generate_keypair();
    let net_sig = sign_manifest(net_manifest.as_bytes(), &net_sec).unwrap();
    let err = lib
        .install_package(&wasm, net_manifest.as_bytes(), &net_sig, &net_pub)
        .unwrap_err();
    assert!(
        err.to_string().contains("network"),
        "capability violation must name the cap: {err}"
    );

    // Malformed manifest (bad version): rejected before any install.
    let bad = MANIFEST.replace("\"1.0.0\"", "\"1.0\"");
    let bad_manifest: Result<PluginManifest, _> = serde_json::from_str(&bad);
    match bad_manifest {
        Ok(m) => {
            let err = hexforge_plugin_host::validate_manifest(&m).unwrap_err();
            assert!(err.to_string().contains("version"), "{err}");
        }
        Err(e) => panic!("test fixture broke JSON shape: {e}"),
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// Component pinned to a contract version the host does not speak.
/// Well-formed and loadable — only the version gate rejects it.
const WRONG_VERSION_WAT: &str = include_str!("data/wit_wrong_version.component.wat");

fn wrong_version_package() -> (Vec<u8>, Vec<u8>, String, String) {
    let wasm = wat::parse_str(WRONG_VERSION_WAT).expect("fixture WAT assembles");
    let bound: PluginManifest = serde_json::from_str(MANIFEST).unwrap();
    let manifest_text = MANIFEST.replace(&bound.wasm_sha256.unwrap(), &sha256_hex(&wasm));
    let (pubkey, secret) = generate_keypair();
    let sig = sign_manifest(manifest_text.as_bytes(), &secret).unwrap();
    (wasm, manifest_text.into_bytes(), sig, pubkey)
}

#[test]
fn sdk_wrong_contract_version_is_incompatible() {
    use hexforge_plugin_host::PluginError;

    let (wasm, manifest, sig, pubkey) = wrong_version_package();
    let root = fresh_root();

    // Install refuses: the failure is version/ABI, not signature or bytes.
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let err = lib
        .install_package(&wasm, &manifest, &sig, &pubkey)
        .unwrap_err();
    assert!(
        matches!(err, PluginError::Incompatible(_)),
        "expected Incompatible, got: {err}"
    );

    // Discovery of a staged package reports Incompatible (never Verified).
    let pkg = root.join("example.wit-uppercase");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("manifest.json"), &manifest).unwrap();
    std::fs::write(pkg.join("manifest.json.sig"), &sig).unwrap();
    std::fs::write(pkg.join("manifest.json.pub"), &pubkey).unwrap();
    std::fs::write(pkg.join("plugin.wasm"), &wasm).unwrap();
    let found: Vec<_> = lib
        .discover()
        .into_iter()
        .filter(|d| d.id == "example.wit-uppercase")
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status, PluginStatus::Incompatible);
    assert!(found[0].instance.is_none());

    let _ = std::fs::remove_dir_all(&root);
}
