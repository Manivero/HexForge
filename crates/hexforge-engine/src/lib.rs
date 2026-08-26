//! hexforge-engine — доменно-нейтральный исполнитель HexForge.
//!
//! Здесь живёт всё, что нужно и GUI-шеллу (`src-tauri`), и CLI
//! (`hexforge-cli`, FR-7.3): состояние исполнения (`AppState`),
//! планировщик цепочки узлов (`scheduler::execute_chain`),
//! lineage-реплей снапшотов (`scheduler::replay_snapshot`) и зеркало
//! IPC-ошибок (`error::HexForgeError`). Крейт не знает о Tauri: прогресс
//! отдаётся через обычный callback, а не через `tauri::AppHandle`.
//!
//! Правило зависимостей (docs/04 §1, уточнение §6):
//! `engine ──▶ core, ops, stream` — однонаправленно, без циклов.

pub mod error;
pub mod graph_dto;
pub mod scheduler;
pub mod state;

pub use error::{HexForgeError, HexForgeErrorKind, HexForgeResult};
pub use state::{
    AppState, CancellationToken, SourceEntry, SourceStore, WriteRegionError,
};
