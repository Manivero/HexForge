//! Состояние Tauri-приложения. Единственное место, где домен (`hexforge-core`)
//! встречается с рантаймом (Tauri `State`, потокобезопасные примитивы).
//! `hexforge-core` сам по себе ничего не знает про `parking_lot`/`tauri::State`.

use hexforge_core::{Graph, History, TransformRegistry};
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

/// Источник байтов, на который фронтенд ссылается через непрозрачный
/// `SourceHandle` (см. `05-IPC-CONTRACT.md`). Никогда не сериализуется
/// целиком в IPC — только через `preview_bytes` с явным диапазоном.
pub enum SourceEntry {
    /// Небольшие литералы и промежуточные результаты — в памяти процесса.
    InMemory(Vec<u8>),
    /// Файлы на диске — memory-mapped, без полной загрузки в RAM (NFR-2).
    Mapped(memmap2::Mmap),
}

impl SourceEntry {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            SourceEntry::InMemory(v) => v.as_slice(),
            SourceEntry::Mapped(m) => &m[..],
        }
    }
}

#[derive(Default)]
pub struct SourceStore {
    entries: HashMap<Uuid, SourceEntry>,
}

impl SourceStore {
    pub fn insert(&mut self, entry: SourceEntry) -> Uuid {
        let handle = Uuid::new_v4();
        self.entries.insert(handle, entry);
        handle
    }

    pub fn get(&self, handle: &Uuid) -> Option<&SourceEntry> {
        self.entries.get(handle)
    }

    pub fn release(&mut self, handle: &Uuid) -> bool {
        self.entries.remove(handle).is_some()
    }
}

pub struct AppState {
    pub registry: TransformRegistry,
    pub sources: RwLock<SourceStore>,
    pub graph: RwLock<Graph>,
    /// Time-Travel история (FR-4): пишется при каждом успешном `run_node`,
    /// читается `list_snapshots`. Отдельный лок — история и источник байтов
    /// имеют разные времена жизни и никогда не блокируются вложенно.
    pub history: RwLock<History>,
}

impl AppState {
    pub fn new(registry: TransformRegistry) -> Self {
        Self {
            registry,
            sources: RwLock::new(SourceStore::default()),
            graph: RwLock::new(Graph::new()),
            history: RwLock::new(History::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_store_insert_get_release_roundtrip() {
        let mut store = SourceStore::default();
        let handle = store.insert(SourceEntry::InMemory(vec![1, 2, 3]));

        let entry = store.get(&handle).expect("entry must exist after insert");
        assert_eq!(entry.as_bytes(), &[1, 2, 3]);

        assert!(store.release(&handle));
        assert!(store.get(&handle).is_none());
        assert!(!store.release(&handle), "double release must report false");
    }

    #[test]
    fn source_store_handles_are_unique() {
        let mut store = SourceStore::default();
        let a = store.insert(SourceEntry::InMemory(vec![0]));
        let b = store.insert(SourceEntry::InMemory(vec![0]));
        assert_ne!(a, b, "each insert must mint a fresh handle");
    }

    #[test]
    fn app_state_starts_empty_and_consistent() {
        let state = AppState::new(hexforge_ops::build_registry());
        assert!(state.history.read().snapshots.is_empty());
        assert!(state.graph.read().nodes.is_empty());
        assert!(state.sources.read().get(&Uuid::new_v4()).is_none());
    }
}
