//! Состояние Tauri-приложения. Единственное место, где домен (`hexforge-core`)
//! встречается с рантаймом (Tauri `State`, потокобезопасные примитивы).
//! `hexforge-core` сам по себе ничего не знает про `parking_lot`/`tauri::State`.

use hexforge_core::{Graph, TransformRegistry};
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
}

impl AppState {
    pub fn new(registry: TransformRegistry) -> Self {
        Self {
            registry,
            sources: RwLock::new(SourceStore::default()),
            graph: RwLock::new(Graph::new()),
        }
    }
}
