//! Plugin SDK tooling: keygen → manifest → sign → validate (CI-friendly).

use hexforge_cli::{
    plugin_bind_artifact, plugin_install, plugin_keygen, plugin_new, plugin_sign_manifest,
    plugin_validate_manifest,
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

#[test]
fn plugin_new_scaffolds_valid_unbound_project() {
    let dir = std::env::temp_dir().join(format!("hexforge-new-{}", uuid::Uuid::new_v4()));
    let dir_s = dir.to_string_lossy().into_owned();

    let msg = plugin_new(&dir_s, Some("acme.demo"), Some("Demo")).unwrap();
    assert!(msg.contains("acme.demo"), "{msg}");
    for rel in [
        "Cargo.toml",
        "src/lib.rs",
        "wit/plugin.wit",
        "manifest.json",
        "README.md",
    ] {
        assert!(dir.join(rel).is_file(), "missing {rel}");
    }
    // Re-issued manifest: fresh id/name, dev version, no stale binding.
    let manifest_text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_json["id"], "acme.demo");
    assert_eq!(manifest_json["name"], "Demo");
    assert_eq!(manifest_json["version"], "0.1.0");
    assert!(manifest_json.get("wasm_sha256").is_none());
    // Scaffold validates before any build/bind/sign step.
    plugin_validate_manifest(&dir.join("manifest.json").to_string_lossy()).unwrap();

    // Scaffolding never overwrites existing work.
    let err = plugin_new(&dir_s, Some("acme.other"), None).unwrap_err();
    assert!(err.contains("non-empty"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}

const TEMPLATE_WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const TEMPLATE_MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");

#[test]
fn plugin_install_template_artifact_end_to_end() {
    // Real developer flow on the committed template build: sign the bound
    // manifest with an ephemeral key, install headless, rediscover.
    let work = std::env::temp_dir().join(format!("hexforge-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).unwrap();
    let wasm_path = work.join("plugin.wasm").to_string_lossy().into_owned();
    let manifest_path = work.join("manifest.json").to_string_lossy().into_owned();
    std::fs::write(&wasm_path, TEMPLATE_WASM).unwrap();
    std::fs::write(&manifest_path, TEMPLATE_MANIFEST).unwrap();

    let (pubkey, secret) = plugin_keygen();
    let sig = plugin_sign_manifest(&manifest_path, &secret).unwrap();
    std::fs::write(format!("{manifest_path}.sig"), &sig).unwrap();
    std::fs::write(format!("{manifest_path}.pub"), &pubkey).unwrap();

    // Sidecar path (no --sig/--pub flags).
    let root = work.join("library").to_string_lossy().into_owned();
    let msg = plugin_install(&wasm_path, &manifest_path, &root, None, None).unwrap();
    assert!(msg.contains("example.wit-uppercase"), "{msg}");
    assert!(msg.contains("Verified"), "{msg}");

    let library = hexforge_plugin_host::store::PluginLibrary::new(
        std::path::PathBuf::from(&root),
        Vec::new(),
    );
    let verified = library.verified_instances();
    assert_eq!(verified.len(), 1);
    assert_eq!(verified[0].manifest.id, "example.wit-uppercase");

    // Wrong key fails closed with a developer-readable error.
    let (other_pub, _) = plugin_keygen();
    let err = plugin_install(
        &wasm_path,
        &manifest_path,
        &root,
        Some(&sig),
        Some(&other_pub),
    )
    .unwrap_err();
    assert!(err.contains("signature"), "{err}");

    let _ = std::fs::remove_dir_all(&work);
}
