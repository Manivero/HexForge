//! Plugin SDK tooling: keygen → manifest → sign → validate (CI-friendly).

use hexforge_cli::{
    plugin_bind_artifact, plugin_keygen, plugin_sign_manifest, plugin_validate_manifest,
};

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

#[test]
fn plugin_bind_before_sign_matches_library_and_pins_bytes() {
    // bind пишет wasm_sha256 артефакта; подпись — ПОСЛЕ bind по тем байтам,
    // что ставятся. Без новых зависимостей: сверяем CLI с библиотечным
    // хелпером и проверяем, что привязка видит подмену байтов.
    let wasm: &[u8] = b"\0asm\x01\0\0\0"; // минимальный пустой модуль
    let mut wasm_path = std::env::temp_dir();
    wasm_path.push(format!(
        "hexforge-plugin-test-{}.wasm",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&wasm_path, wasm).unwrap();
    let manifest_path = write_temp_manifest(GOOD_MANIFEST);

    let msg = plugin_bind_artifact(&manifest_path, &wasm_path.to_string_lossy()).unwrap();
    assert!(msg.contains("wasm_sha256="), "{msg}");

    let bound = std::fs::read(&manifest_path).unwrap();
    let bound_json: serde_json::Value = serde_json::from_slice(&bound).unwrap();
    let pinned = bound_json["wasm_sha256"].as_str().unwrap().to_string();
    assert_eq!(pinned.len(), 64);

    // Тот же результат даёт библиотечный хелпер (install сверяется так же).
    let expected =
        hexforge_plugin_host::bind_wasm_artifact(GOOD_MANIFEST.as_bytes(), wasm).unwrap();
    let expected_json: serde_json::Value = serde_json::from_slice(&expected).unwrap();
    assert_eq!(pinned, expected_json["wasm_sha256"].as_str().unwrap());

    // Подмена артефакта после bind меняет привязку — install это отклонит.
    let tampered = [wasm, b"\0".as_slice()].concat();
    let rebound =
        hexforge_plugin_host::bind_wasm_artifact(GOOD_MANIFEST.as_bytes(), &tampered).unwrap();
    let rebound_json: serde_json::Value = serde_json::from_slice(&rebound).unwrap();
    assert_ne!(pinned, rebound_json["wasm_sha256"].as_str().unwrap());

    let _ = std::fs::remove_file(&manifest_path);
    let _ = std::fs::remove_file(&wasm_path);
}
