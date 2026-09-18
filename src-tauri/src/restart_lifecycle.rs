//! Restart lifecycle через настоящий Tauri IPC-boundary (FR-6 persistence).
//!
//! Цепочка: Install → persist → close process → start process → discovery
//! → signature verification → registry → execute. Каждый шаг выполняется в
//! отдельном OS-процессе, общем только on-disk root.
//!
//! # Честно покрыто
//! - install/reject через настоящие Tauri-команды: тот же `generate_handler`,
//!   та же JSON-десериализация аргументов и сериализация ответов/ошибок, что
//!   использует WebView (`tauri::test` mock-runtime, WebView не нужен).
//! - persistence между отдельными process lifetimes (child-процессы через
//!   `current_exe`, связь только через temp-dir на диске).
//! - discovery + повторная verify подписи после рестарта, статус в ответе
//!   `list_plugins` (тот самый DTO, что рисует PluginPanel).
//! - grants persistence (revoke/grant через IPC, проверка после рестарта).
//! - execution rediscovered-артефакта через зарегистрированный в реестре
//!   transform (зеркало `setup` из `main.rs`).
//! - tamper wasm / manifest / missing artifact после рестарта → fail closed.
//!
//! # Ручная проверка (не автоматизируется здесь)
//! - Рендер WebView и JS-транспорт IPC поверх `invoke` (покрыт FE-parity
//!   тестами `fe-tests/plugins.test.mjs` + golden-тестом DTO в `commands.rs`).
//! - Реальный app-data путь (тест использует изолированный temp root;
//!   резолв `resolve_plugin_library` тот же, меняется только базовый dir).
//! - Жизненный цикл окна и конкурентные инстансы приложения.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const PLUGIN_ID: &str = "acme.restart";
const PLUGIN_VERSION: &str = "1.0.0";
// Пустой core-модуль: рантайм исполняет его как echo (конвенция проверена
// `persistence.rs`: ECHO_WAT собирается ровно в эти байты).
const ECHO_MODULE: &[u8] = b"\0asm\x01\0\0\0";
// Тот же модуль + секция памяти: валиден, но байты другие → привязка
// `wasm_sha256` после подмены не сходится (ожидаем `invalid`, не Verified).
const TAMPERED_MODULE: &[u8] = b"\0asm\x01\0\0\0\x05\x03\x01\0\x01";

const ENV_ROOT: &str = "HEXFORGE_RESTART_ROOT";
const ENV_STAGE: &str = "HEXFORGE_RESTART_STAGE";

/// Temp root из env; `None` = фаза запущена не оркестратором → skip.
fn phase_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

/// Собирает подписанный install-пакет SDK-потоком: author → bind → sign.
fn signed_package(
    requested: &[&str],
    granted: &[&str],
    wasm: &[u8],
) -> (Vec<u8>, Vec<u8>, String, String) {
    let req = requested
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let gr = granted
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let raw = format!(
        "{{\n  \"id\": \"{PLUGIN_ID}\",\n  \"name\": \"Restart Probe\",\n  \"version\": \"{PLUGIN_VERSION}\",\n  \"author\": \"HexForge\",\n  \"requested_capabilities\": [{req}],\n  \"granted_capabilities\": [{gr}]\n}}"
    );
    let manifest = hexforge_plugin_host::bind_wasm_artifact(raw.as_bytes(), wasm).unwrap();
    let (pubkey, secret) = hexforge_plugin_host::generate_keypair();
    let sig = hexforge_plugin_host::sign_manifest(&manifest, &secret).unwrap();
    (wasm.to_vec(), manifest, sig, pubkey)
}

fn write_stage(stage: &Path) -> (PathBuf, PathBuf) {
    let pkg = signed_package(&["network"], &["network"], ECHO_MODULE);
    let wasm_path = stage.join("plugin.wasm");
    let manifest_path = stage.join("manifest.json");
    std::fs::write(&wasm_path, &pkg.0).unwrap();
    std::fs::write(&manifest_path, &pkg.1).unwrap();
    // Сайдкары рядом с манифестом: их читает install_plugin, без них —
    // fail-closed (нет unsigned dev-mode).
    std::fs::write(stage.join("manifest.json.sig"), &pkg.2).unwrap();
    std::fs::write(stage.join("manifest.json.pub"), &pkg.3).unwrap();
    (wasm_path, manifest_path)
}

/// Один IPC-вызов через настоящий Tauri-диспетчер: та же JSON-граница, что
/// видит WebView (сериализация аргументов/ответов и кодов ошибок).
fn invoke_on(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let req = tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    };
    tauri::test::get_ipc_response(webview, req)
        .map(|body| body.deserialize::<serde_json::Value>().unwrap())
}

/// Свежее приложение с реальными состояниями (как `main`: AppState +
/// PluginRuntime + PluginLibrary) и прод-обработчиком plugin-команд.
fn with_mock_ipc<T>(
    library: hexforge_plugin_host::store::PluginLibrary,
    f: impl FnOnce(
        &dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, serde_json::Value>,
    ) -> T,
) -> T {
    let runtime = Arc::new(hexforge_plugin_host::PluginRuntime::new(None).unwrap());
    let registry = hexforge_ops::build_registry();
    let app = tauri::test::mock_builder()
        .manage(Arc::new(hexforge_engine::state::AppState::new(registry)))
        .manage(runtime)
        .manage(library)
        .invoke_handler(tauri::generate_handler![
            crate::commands::install_plugin,
            crate::commands::list_plugins,
            crate::commands::grant_capability,
            crate::commands::revoke_capability,
        ])
        .build(tauri::generate_context!())
        .expect("mock Tauri app builds");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("mock webview builds");
    f(&|cmd, args| invoke_on(&webview, cmd, args))
}

// --- Фаза A: install с чужой подписью отклоняется через IPC ----------------
// Проверяет: подпись до установки, actionable-ошибка через UI-границу.
#[test]
fn phase_install_rejects_bad_signature() {
    let (Some(root), Some(stage)) = (phase_dir(ENV_ROOT), phase_dir(ENV_STAGE)) else {
        return;
    };
    let lib = hexforge_plugin_host::store::PluginLibrary::new(root, Vec::new());
    with_mock_ipc(
        lib,
        |ipc: &dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, serde_json::Value>| {
            let (wasm_path, manifest_path) = write_stage(&stage);
            // Чужой ключ: те же байты, подпись не их → install обязан отказать
            // до persist (ничего не должно появиться в library root).
            let (_, other_secret) = hexforge_plugin_host::generate_keypair();
            let manifest = std::fs::read(&manifest_path).unwrap();
            let bad_sig = hexforge_plugin_host::sign_manifest(&manifest, &other_secret).unwrap();
            std::fs::write(stage.join("manifest.json.sig"), &bad_sig).unwrap();
            let err = ipc(
                "install_plugin",
                serde_json::json!({"req": {
                    "wasmPath": wasm_path.to_string_lossy(),
                    "manifestPath": manifest_path.to_string_lossy(),
                }}),
            )
            .expect_err("foreign signature must be rejected");
            let msg = err.to_string().to_lowercase();
            assert!(msg.contains("signature"), "actionable error, got: {err}");
        },
    );
}

// --- Фаза B: install → list → revoke → grant через IPC ----------------------
// Проверяет: install через UI-границу, verified-DTO, grants туда-обратно.
#[test]
fn phase_install_grant_list_via_ipc() {
    let (Some(root), Some(stage)) = (phase_dir(ENV_ROOT), phase_dir(ENV_STAGE)) else {
        return;
    };
    let lib = hexforge_plugin_host::store::PluginLibrary::new(root, Vec::new());
    with_mock_ipc(
        lib,
        |ipc: &dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, serde_json::Value>| {
            let (wasm_path, manifest_path) = write_stage(&stage);
            let install_args = serde_json::json!({"req": {
                "wasmPath": wasm_path.to_string_lossy(),
                "manifestPath": manifest_path.to_string_lossy(),
            }});
            let dto = ipc("install_plugin", install_args).expect("install via IPC");
            assert_eq!(dto["id"], PLUGIN_ID);
            assert_eq!(dto["version"], PLUGIN_VERSION);
            assert_eq!(dto["status"], "verified");
            assert_eq!(dto["signatureValid"], true);

            let list = ipc("list_plugins", serde_json::json!({})).expect("list via IPC");
            assert_eq!(list.as_array().unwrap().len(), 1);
            assert_eq!(list[0]["status"], "verified");

            // revoke → grant: оба направления через IPC, как жмёт UI.
            let revoke =
                serde_json::json!({"req": {"pluginId": PLUGIN_ID, "capability": "network"}});
            assert_eq!(ipc("revoke_capability", revoke).expect("revoke"), true);
            let list = ipc("list_plugins", serde_json::json!({})).unwrap();
            assert!(list[0]["grantedCapabilities"]
                .as_array()
                .unwrap()
                .is_empty());
            let grant =
                serde_json::json!({"req": {"pluginId": PLUGIN_ID, "capability": "network"}});
            assert_eq!(ipc("grant_capability", grant).expect("grant"), true);
            let list = ipc("list_plugins", serde_json::json!({})).unwrap();
            let granted = list[0]["grantedCapabilities"].as_array().unwrap();
            assert_eq!(granted, &vec![serde_json::Value::from("network")]);
        },
    );
}

// --- Фаза C: discover + execute после рестарта --------------------------------
// Новый процесс, новые runtime/состояния: всё только с диска. Зеркалит
// `setup` из main.rs (verified → register), затем исполняет
// зарегистрированный transform и сверяет результат с дорестартным.
#[test]
fn phase_verify_execute_after_restart() {
    let Some(root) = phase_dir(ENV_ROOT) else {
        return;
    };
    // Прямой discovery-слой: подпись перепроверена, гранты восстановлены.
    let lib = hexforge_plugin_host::store::PluginLibrary::new(root.clone(), Vec::new());
    let found = lib.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].status,
        hexforge_plugin_host::store::PluginStatus::Verified
    );
    assert!(found[0].instance.is_some());
    let instances = lib.verified_instances();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].manifest.id, PLUGIN_ID);
    assert_eq!(
        instances[0].manifest.granted_capabilities,
        vec!["network".to_string()]
    );

    // UI-граница: тот же verified-DTO после рестарта.
    with_mock_ipc(
        lib,
        |ipc: &dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, serde_json::Value>| {
            let list = ipc("list_plugins", serde_json::json!({})).expect("list");
            assert_eq!(list.as_array().unwrap().len(), 1);
            assert_eq!(list[0]["id"], PLUGIN_ID);
            assert_eq!(list[0]["status"], "verified");
            assert_eq!(list[0]["signatureValid"], true);
        },
    );

    // Реестр + исполнение: точь-в-точь регистрация из `setup`.
    let lib2 = hexforge_plugin_host::store::PluginLibrary::new(root, Vec::new());
    let runtime = Arc::new(hexforge_plugin_host::PluginRuntime::new(None).unwrap());
    let app_state = hexforge_engine::state::AppState::new(hexforge_ops::build_registry());
    let instances = lib2.verified_instances();
    assert_eq!(instances.len(), 1);
    let registered: &'static dyn hexforge_core::Transform = {
        let pt = runtime
            .clone()
            .as_transform(instances[0].clone())
            .expect("loads");
        let leaked: Box<dyn hexforge_core::Transform> = Box::new(pt);
        Box::leak(leaked)
    };
    app_state
        .register_plugin(registered)
        .expect("single test plugin registers");
    let input = b"restart-probe-input";
    let out = registered
        .apply(
            input.as_slice().into(),
            &serde_json::json!({}),
            &hexforge_core::transform::NullExecutionContext,
        )
        .expect("registered plugin executes");
    assert_eq!(&*out, input);
}

// --- Фазы D/E/F: tamper после рестарта → fail closed ---------------------------
fn assert_single_rejected(root: &Path, status: &str) {
    let lib = hexforge_plugin_host::store::PluginLibrary::new(root.to_path_buf(), Vec::new());
    let found = lib.discover();
    assert_eq!(found.len(), 1);
    assert_ne!(
        found[0].status,
        hexforge_plugin_host::store::PluginStatus::Verified
    );
    assert!(lib.verified_instances().is_empty(), "nothing registrable");
    with_mock_ipc(
        lib,
        |ipc: &dyn Fn(&str, serde_json::Value) -> Result<serde_json::Value, serde_json::Value>| {
            let list = ipc("list_plugins", serde_json::json!({})).expect("list");
            assert_eq!(list.as_array().unwrap().len(), 1);
            assert_eq!(list[0]["status"], status);
            assert_eq!(list[0]["signatureValid"], false);
        },
    );
}

/// Подмена `plugin.wasm`: привязка не сходится → `invalid`, не исполняется.
#[test]
fn phase_tampered_wasm_rejected() {
    let Some(root) = phase_dir(ENV_ROOT) else {
        return;
    };
    assert_single_rejected(&root, "invalid");
}

/// Подмена `manifest.json` (валидный JSON, чужие байты): подпись не
/// сходится → `invalid`.
#[test]
fn phase_tampered_manifest_rejected() {
    let Some(root) = phase_dir(ENV_ROOT) else {
        return;
    };
    assert_single_rejected(&root, "invalid");
}

/// Удалённый артефакт: нечего исполнять → `unavailable`.
#[test]
fn phase_missing_artifact_rejected() {
    let Some(root) = phase_dir(ENV_ROOT) else {
        return;
    };
    assert_single_rejected(&root, "unavailable");
}

// --- Оркестратор: один процесс на фазу, связь только через диск ---------------
fn current_test_exe() -> PathBuf {
    std::env::current_exe().expect("test binary path")
}

fn spawn_phase(name: &str, root: &Path, stage: &Path) {
    let status = Command::new(current_test_exe())
        .arg("--exact")
        .arg(format!("restart_lifecycle::{name}"))
        .arg("--nocapture")
        .env(ENV_ROOT, root)
        .env(ENV_STAGE, stage)
        .status()
        .unwrap_or_else(|e| panic!("cannot spawn phase {name}: {e}"));
    assert!(status.success(), "phase {name} failed: {status}");
}

/// Оркестратор полного lifecycle: каждая фаза — отдельный OS-процесс,
/// связь только через temp-dir на диске. Падает, если любая фаза падает.
#[test]
fn restart_full_lifecycle() {
    if std::env::var_os(ENV_ROOT).is_some() {
        return; // страховка: в child-процессах работают только фазы
    }
    let tag = uuid::Uuid::new_v4().to_string();
    let root = std::env::temp_dir().join(format!("hexforge-restart-{tag}"));
    let stage = std::env::temp_dir().join(format!("hexforge-restage-{tag}"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&stage).unwrap();

    // A: чужая подпись отклоняется до persist.
    spawn_phase("phase_install_rejects_bad_signature", &root, &stage);
    // B: install + revoke/grant через IPC.
    spawn_phase("phase_install_grant_list_via_ipc", &root, &stage);

    // On-disk инварианты после install: пакет + гранты, без staging-мусора.
    let pkg_dir = root.join(PLUGIN_ID);
    for name in [
        "plugin.wasm",
        "manifest.json",
        "manifest.json.sig",
        "manifest.json.pub",
        "grants.json",
    ] {
        assert!(pkg_dir.join(name).is_file(), "persisted {name}");
    }
    let grants = std::fs::read_to_string(pkg_dir.join("grants.json")).unwrap();
    assert!(grants.contains("network"), "grants persisted: {grants}");
    let debris: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .collect();
    assert!(debris.is_empty(), "no staging debris: {debris:?}");

    // C: новый процесс видит verified + гранты и исполняет.
    spawn_phase("phase_verify_execute_after_restart", &root, &stage);

    // D: подмена wasm после рестарта → invalid, исполнения нет.
    let wasm_on_disk = std::fs::read(pkg_dir.join("plugin.wasm")).unwrap();
    std::fs::write(pkg_dir.join("plugin.wasm"), TAMPERED_MODULE).unwrap();
    spawn_phase("phase_tampered_wasm_rejected", &root, &stage);

    // E: подмена manifest (валидный JSON, чужие байты) → invalid.
    std::fs::write(pkg_dir.join("plugin.wasm"), &wasm_on_disk).unwrap();
    let manifest_on_disk = std::fs::read(pkg_dir.join("manifest.json")).unwrap();
    let tampered_manifest = String::from_utf8(manifest_on_disk.clone())
        .unwrap()
        .replacen(PLUGIN_VERSION, "9.9.9", 1);
    assert_ne!(tampered_manifest.as_bytes(), manifest_on_disk.as_slice());
    std::fs::write(pkg_dir.join("manifest.json"), &tampered_manifest).unwrap();
    spawn_phase("phase_tampered_manifest_rejected", &root, &stage);

    // F: удалённый артефакт → unavailable.
    std::fs::write(pkg_dir.join("manifest.json"), &manifest_on_disk).unwrap();
    std::fs::remove_file(pkg_dir.join("plugin.wasm")).unwrap();
    spawn_phase("phase_missing_artifact_rejected", &root, &stage);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&stage);
}
