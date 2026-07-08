// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod state;

use state::AppState;

fn main() {
    // Реестр операций строится один раз при старте из всех `Transform`,
    // собранных `inventory` в `hexforge-ops` на этапе линковки —
    // ни один встроенный оператор не требует правки этого файла (FR-3.1).
    let registry = hexforge_ops::build_registry();
    eprintln!("[hexforge-core] initialized with {} operations", registry.len());

    tauri::Builder::default()
        .manage(AppState::new(registry))
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::list_operations,
            commands::open_file,
            commands::create_literal_source,
            commands::preview_bytes,
            commands::release_source,
            commands::set_graph,
            commands::run_node,
            commands::cancel_node,
            commands::list_snapshots,
            commands::list_plugins,
        ])
        .run(tauri::generate_context!())
        .expect("error while running HexForge");
}
