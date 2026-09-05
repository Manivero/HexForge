//! Plugin SDK tooling: keygen → manifest → sign → validate (CI-friendly).

use hexforge_cli::{plugin_keygen, plugin_sign_manifest, plugin_validate_manifest};

fn write_temp_manifest(body: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "hexforge-plugin-test-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

const GOOD_MANIFEST: &str = r#"{
  "id": "acme.uppercase",
  "name": "Acme Uppercase",
  "version": "1.0.0",
  "author": "Acme",
  "requested_capabilities": [],
  "granted_capabilities": []
}"#;

#[test]
fn plugin_toolchain_keygen_sign_validate_roundtrip() {
    let (pubkey_hex, signing_key_hex) = plugin_keygen();
    assert_eq!(pubkey_hex.len(), 64);
    assert_eq!(signing_key_hex.len(), 64);

    let manifest_path = write_temp_manifest(GOOD_MANIFEST);
    let msg = plugin_validate_manifest(&manifest_path).unwrap();
    assert!(msg.contains("acme.uppercase"), "{msg}");
    assert!(msg.contains("1.0.0"), "{msg}");

    let sig_hex = plugin_sign_manifest(&manifest_path, &signing_key_hex).unwrap();
    assert_eq!(sig_hex.len(), 128);

    // Подпись проверяется хостом (та же проверка, что в install).
    let bytes = std::fs::read(&manifest_path).unwrap();
    assert!(hexforge_plugin_host::verify_signature(&bytes, &sig_hex, &pubkey_hex).unwrap());
    // Чужой ключ подпись не подтверждает.
    let (other_pubkey, _) = plugin_keygen();
    assert!(!hexforge_plugin_host::verify_signature(&bytes, &sig_hex, &other_pubkey).unwrap());

    let _ = std::fs::remove_file(&manifest_path);
}

#[test]
fn plugin_validate_rejects_bad_manifest() {
    let bad = GOOD_MANIFEST.replace("\"1.0.0\"", "\"1.0\"");
    let path = write_temp_manifest(&bad);
    let err = plugin_validate_manifest(&path).unwrap_err();
    assert!(err.contains("version"), "{err}");
    let _ = std::fs::remove_file(&path);

    let missing = write_temp_manifest("{ not json");
    let err = plugin_validate_manifest(&missing).unwrap_err();
    assert!(err.contains("valid manifest"), "{err}");
    let _ = std::fs::remove_file(&missing);
}

#[test]
fn plugin_sign_rejects_empty_key_and_missing_file() {
    let path = write_temp_manifest(GOOD_MANIFEST);
    let err = plugin_sign_manifest(&path, "  ").unwrap_err();
    assert!(err.contains("signing key"), "{err}");
    let err = plugin_sign_manifest(&path, "zz").unwrap_err();
    assert!(err.contains("cannot sign"), "{err}");
    let err =
        plugin_sign_manifest("C:/nonexistent-hexforge/manifest.json", &"0".repeat(64)).unwrap_err();
    assert!(err.contains("cannot read manifest"), "{err}");
    let _ = std::fs::remove_file(&path);
}
