// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use hexforge_engine::state;

use state::AppState;

fn main() {
    // Реестр операций строится один раз при старте из всех `Transform`,
    // собранных `inventory` в `hexforge-ops` на этапе линковки —
    // ни один встроенный оператор не требует правки этого файла (FR-3.1).
    let mut registry = hexforge_ops::build_registry();
    eprintln!(
        "[hexforge-core] initialized with {} operations",
        registry.len()
    );

    // Plugin host: discover plugins in `./plugins` and register their transforms (FR-6).
    let plugin_runtime = std::sync::Arc::new(
        hexforge_plugin_host::PluginRuntime::new(None).expect("plugin runtime init failed"),
    );
    let plugin_instances = hexforge_plugin_host::list_plugins();
    eprintln!(
        "[hexforge-plugin-host] discovered {} plugin(s)",
        plugin_instances.len()
    );
    for inst in plugin_instances {
        match plugin_runtime.clone().as_transform(inst.clone()) {
            Ok(pt) => {
                let leaked: Box<dyn hexforge_core::Transform> = Box::new(pt);
                let static_ref: &'static dyn hexforge_core::Transform = Box::leak(leaked);
                let id = static_ref.id().to_string();
                registry.register(static_ref);
                eprintln!("[hexforge-plugin-host] registered plugin transform: {id}");
            }
            Err(e) => {
                eprintln!(
                    "[hexforge-plugin-host] failed to load plugin {}: {e}",
                    inst.manifest.id
                );
            }
        }
    }

    // AppState управляется через Arc: async-команда run_node обязана
    // передать владение состоянием в blocking-пул (spawn_blocking требует
    // 'static), не копируя само состояние.
    tauri::Builder::default()
        .manage(std::sync::Arc::new(AppState::new(registry)))
        .manage(plugin_runtime)
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
