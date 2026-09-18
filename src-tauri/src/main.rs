// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
#[cfg(test)]
mod restart_lifecycle;

use hexforge_engine::state;
use std::path::PathBuf;
use tauri::Manager;

use state::AppState;

/// Resolves the persistent plugin library: `HEXFORGE_PLUGINS_DIR` wins when
/// set (tests / portable installs), otherwise the platform app-data dir
/// (`com.hexforge.app`), so installs survive restarts AND cwd changes.
/// The repo-local `./plugins` stays as a read-only dev root — example
/// plugins keep working without being copied into the writable store.
fn resolve_plugin_library<R: tauri::Runtime>(
    app: &tauri::App<R>,
) -> hexforge_plugin_host::store::PluginLibrary {
    let writable = std::env::var(hexforge_plugin_host::store::PLUGINS_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            app.path()
                .app_data_dir()
                .map(|d| d.join("plugins"))
                .unwrap_or_else(|_| PathBuf::from("./plugins"))
        });
    eprintln!(
        "[hexforge-plugin-host] library root: {}",
        writable.display()
    );
    hexforge_plugin_host::store::PluginLibrary::new(writable, vec![PathBuf::from("./plugins")])
}

fn main() {
    // Реестр операций строится один раз при старте из всех `Transform`,
    // собранных `inventory` в `hexforge-ops` на этапе линковки —
    // ни один встроенный оператор не требует правки этого файла (FR-3.1).
    let registry = hexforge_ops::build_registry();
    eprintln!(
        "[hexforge-core] initialized with {} operations",
        registry.len()
    );

    // Plugin host runtime (FR-6). Discovery + registration happen in
    // `setup`, where the app-data library root is resolvable; installs land
    // in that persistent root and are re-verified on every later startup.
    let plugin_runtime = std::sync::Arc::new(
        hexforge_plugin_host::PluginRuntime::new(None).expect("plugin runtime init failed"),
    );

    // AppState управляется через Arc: async-команда run_node обязана
    // передать владение состоянием в blocking-пул (spawn_blocking требует
    // 'static), не копируя само состояние.
    tauri::Builder::default()
        .manage(std::sync::Arc::new(AppState::new(registry)))
        .manage(plugin_runtime)
        .setup(|app| {
            let library = resolve_plugin_library(app);
            let state = app.state::<std::sync::Arc<AppState>>();
            let runtime = app
                .state::<std::sync::Arc<hexforge_plugin_host::PluginRuntime>>()
                .inner()
                .clone();
            let mut verified = 0usize;
            for inst in library.verified_instances() {
                match runtime.clone().as_transform(inst.clone()) {
                    Ok(pt) => {
                        let leaked: Box<dyn hexforge_core::Transform> = Box::new(pt);
                        let static_ref: &'static dyn hexforge_core::Transform = Box::leak(leaked);
                        let id = static_ref.id().to_string();
                        match state.register_plugin(static_ref) {
                            Ok(()) => {
                                verified += 1;
                                eprintln!(
                                    "[hexforge-plugin-host] registered plugin transform: {id}"
                                );
                            }
                            Err(e) => {
                                eprintln!("[hexforge-plugin-host] skipping plugin transform: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[hexforge-plugin-host] verified plugin failed to load {}: {e}",
                            inst.manifest.id
                        );
                    }
                }
            }
            eprintln!("[hexforge-plugin-host] discovered {verified} verified plugin(s)");
            app.manage(library);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::list_operations,
            commands::open_file,
            commands::create_literal_source,
            commands::preview_bytes,
            commands::release_source,
            commands::patch_source,
            commands::set_graph,
            commands::run_node,
            commands::cancel_node,
            commands::export_recipe,
            commands::import_recipe,
            commands::jump_to_snapshot,
            commands::list_snapshots,
            commands::diff_snapshots,
            commands::import_cyberchef_recipe,
            commands::list_plugins,
            commands::install_plugin,
            commands::grant_capability,
            commands::revoke_capability,
        ])
        .run(tauri::generate_context!())
        .expect("error while running HexForge");
}

#[cfg(test)]
mod resolver_tests {
    use super::resolve_plugin_library;
    use tauri::Manager;

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::generate_context!())
            .expect("mock app builds")
    }

    /// `resolve_plugin_library`: override → app-data → fallback, в одном
    /// потоке последовательно (env process-global, параллелить нельзя).
    #[test]
    fn library_root_resolution() {
        let key = hexforge_plugin_host::store::PLUGINS_DIR_ENV;
        let saved = std::env::var_os(key);

        // 1. Override побеждает: writable root ровно он, библиотека рабочая.
        let override_root =
            std::env::temp_dir().join(format!("hexforge-resolver-{}", uuid::Uuid::new_v4()));
        std::env::set_var(key, &override_root);
        let lib = resolve_plugin_library(&mock_app());
        assert_eq!(lib.writable_dir(), override_root.as_path());
        assert!(lib.discover().is_empty());
        let _ = std::fs::remove_dir_all(&override_root);

        // 2. Без override — прод-путь: app-data/plugins либо fallback.
        std::env::remove_var(key);
        let app = mock_app();
        let lib = resolve_plugin_library(&app);
        match app.path().app_data_dir() {
            Ok(data) => assert_eq!(lib.writable_dir(), data.join("plugins").as_path()),
            Err(_) => assert_eq!(lib.writable_dir(), std::path::Path::new("./plugins")),
        }
        assert!(lib.discover().is_empty());

        if let Some(v) = saved {
            std::env::set_var(key, v);
        }
    }
}
