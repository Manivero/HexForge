//! Plugin discovery trust: only signed + valid + loadable entries are listed.
//!
//! Fail-closed: unsigned copies, tampered manifests, invalid manifests and
//! garbage binaries are skipped so the UI never offers an entry whose
//! `signature_valid` it cannot prove.

use hexforge_plugin_host::{
    generate_keypair, list_plugins_in_dir, sign_manifest, verify_signature, PluginManifest,
};

const EXAMPLE_WAT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/example-uppercase/plugin.wat"
);

fn example_manifest(id: &str) -> PluginManifest {
    PluginManifest {
        id: id.into(),
        name: "Example Uppercase".into(),
        version: "1.0.0".into(),
        author: "HexForge Example".into(),
        requested_capabilities: vec![],
        granted_capabilities: vec![],
    }
}

fn setup_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hexforge-discovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn example_wasm_bytes() -> Vec<u8> {
    let wat = std::fs::read_to_string(EXAMPLE_WAT).expect("example plugin.wat must exist");
    wat::parse_str(&wat).expect("example WAT must assemble")
}

/// Writes `<dir>/<stem>.wasm` + `<stem>.json` (+ sidecars when asked).
/// Returns the manifest bytes as written.
fn write_entry(
    dir: &std::path::Path,
    stem: &str,
    wasm_bytes: &[u8],
    manifest: &PluginManifest,
    sign: bool,
    pretty: bool,
) -> Vec<u8> {
    std::fs::write(dir.join(format!("{stem}.wasm")), wasm_bytes).unwrap();
    let manifest_bytes = if pretty {
        let mut s = serde_json::to_string_pretty(manifest).unwrap();
        s.push('\n');
        s.into_bytes()
    } else {
        serde_json::to_vec(manifest).unwrap()
    };
    std::fs::write(dir.join(format!("{stem}.json")), &manifest_bytes).unwrap();
    if sign {
        let (pubkey_hex, signing_key_hex) = generate_keypair();
        let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).unwrap();
        std::fs::write(dir.join(format!("{stem}.json.sig")), &sig_hex).unwrap();
        std::fs::write(dir.join(format!("{stem}.json.pub")), &pubkey_hex).unwrap();
    }
    manifest_bytes
}

#[test]
fn discovery_lists_only_verifiable_entries() {
    let dir = setup_dir();
    let wasm = example_wasm_bytes();

    // Signed, valid, loadable → listed.
    let good_bytes = write_entry(
        &dir,
        "good",
        &wasm,
        &example_manifest("example.uppercase"),
        true,
        true,
    );

    // Signed manifest, then tampered on disk → skipped.
    write_entry(
        &dir,
        "tampered",
        &wasm,
        &example_manifest("example.tampered"),
        true,
        false,
    );
    std::fs::write(
        dir.join("tampered.json"),
        br#"{"id":"example.tampered","name":"X","version":"9.9.9","author":"Mallory"}"#,
    )
    .unwrap();

    // No sidecars → skipped (not verifiable).
    write_entry(
        &dir,
        "unsigned",
        &wasm,
        &example_manifest("example.unsigned"),
        false,
        false,
    );

    // Signed manifest but garbage binary → skipped (load check).
    write_entry(
        &dir,
        "garbage",
        b"definitely not wasm",
        &example_manifest("example.garbage"),
        true,
        false,
    );

    // Signed but semantically invalid manifest → skipped (validation).
    let mut bad = example_manifest("example.bad");
    bad.version = "1.0".into();
    write_entry(&dir, "badmanifest", &wasm, &bad, true, false);

    // Noise is ignored.
    std::fs::write(dir.join("notes.txt"), b"not a plugin").unwrap();

    let listed = list_plugins_in_dir(&dir);
    assert_eq!(listed.len(), 1, "only the signed entry must be listed");
    let inst = &listed[0];
    assert_eq!(inst.manifest.id, "example.uppercase");
    assert!(!inst.signature_hex.is_empty() && !inst.pubkey_hex.is_empty());

    // The attached sidecars verify over the original manifest bytes,
    // and the path helpers agree with the on-disk layout.
    assert_eq!(
        inst.manifest_path(),
        dir.join("good.json"),
        "manifest pairing convention"
    );
    assert_eq!(inst.signature_path(), dir.join("good.json.sig"));
    assert_eq!(inst.pubkey_path(), dir.join("good.json.pub"));
    assert!(verify_signature(&good_bytes, &inst.signature_hex, &inst.pubkey_hex).unwrap());

    // Pin the UI rule: re-serialized manifest bytes must NOT verify (pretty
    // file ≠ compact serialization), so `list_plugins` has to verify over the
    // original file bytes — exactly what `manifest_path()` gives it.
    let reserialized = serde_json::to_vec(&inst.manifest).unwrap();
    assert_ne!(reserialized, good_bytes, "test file must be pretty-printed");
    assert!(
        !verify_signature(&reserialized, &inst.signature_hex, &inst.pubkey_hex).unwrap_or(false),
        "re-serialized bytes must not verify"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_missing_dir_is_empty() {
    let missing = std::env::temp_dir().join(format!("hexforge-nope-{}", uuid::Uuid::new_v4()));
    assert!(list_plugins_in_dir(&missing).is_empty());
}
