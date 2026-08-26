//! Состояние исполнения HexForge (используется GUI-шеллом и CLI). Единственное место, где домен (`hexforge-core`)
//! встречается с рантаймом (Tauri `State`, потокобезопасные примитивы).
//! `hexforge-core` сам по себе ничего не знает про `parking_lot`/`tauri::State`.

use hexforge_core::{Graph, History, NodeId, TransformRegistry};
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use uuid::Uuid;

/// Токен кооперативной отмены выполнения узла (`cancel_node`): планировщик
/// опрашивает флаг на чекпоинтах между узлами и между чанками.
pub type CancellationToken = Arc<AtomicBool>;

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

    /// Перезаписывает регион существующего InMemory-источника (FR Hex Editor).
    /// Семантика MVP: только точная перезапись в границах — без роста,
    /// без записи в Mapped (файлы на диске остаются read-only).
    pub fn write_region(
        &mut self,
        handle: &Uuid,
        offset: usize,
        data: &[u8],
    ) -> Result<usize, WriteRegionError> {
        let entry = self
            .entries
            .get_mut(handle)
            .ok_or(WriteRegionError::UnknownHandle)?;
        match entry {
            SourceEntry::InMemory(buf) => {
                let end = offset.checked_add(data.len()).ok_or(
                    WriteRegionError::OutOfBounds { size: buf.len(), required_end: usize::MAX },
                )?;
                if end > buf.len() {
                    return Err(WriteRegionError::OutOfBounds {
                        size: buf.len(),
                        required_end: end,
                    });
                }
                buf[offset..end].copy_from_slice(data);
                Ok(buf.len())
            }
            SourceEntry::Mapped(_) => Err(WriteRegionError::ReadOnlyMapped),
        }
    }

    /// Заменяет байты под существующим handle (нужен тестам целостности
    /// replay-реплея); false — handle неизвестен.
    #[cfg(test)]
    pub fn replace(&mut self, handle: Uuid, entry: SourceEntry) -> bool {
        match self.entries.get_mut(&handle) {
            Some(slot) => {
                *slot = entry;
                true
            }
            None => false,
        }
    }
}

/// Ошибки patch_source; маппинг в HexForgeError — на IPC-слое.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRegionError {
    UnknownHandle,
    OutOfBounds { size: usize, required_end: usize },
    ReadOnlyMapped,
}

/// Content-addressed LRU-кэш выходов узлов планировщика (см. scheduler.rs).
/// Ключ — `reproducibility_key(op@ver :: input_hash :: params)`: инвалидация
/// не требуется по построению, вытеснение — только по бюджету байтов.
pub struct OutputCache {
    entries: HashMap<String, CacheEntry>,
    /// Порядок доступа для вытеснения старейших: ключи в порядке вставки,
    /// при hit-переиспользовании порядок не обновляется (детерминизм важнее
    /// точного LRU; бюджет и так ограничивает worst-case память).
    order: VecDeque<String>,
    used_bytes: usize,
    max_bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

struct CacheEntry {
    output: Arc<Vec<u8>>,
}

impl OutputCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            used_bytes: 0,
            max_bytes,
            hits: 0,
            misses: 0,
        }
    }

    /// Число живых записей (диагностика/тесты).
    /// Полная очистка кэша: вызывается при мутации источника (patch_source) —
    /// content-addressed ключи не позволяют точечно найти зависимые записи
    /// (хэш входа меняется), консервативная инвалидация гарантирует, что
    /// кэш никогда не вернёт байты, не соответствующие текущему источнику.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.used_bytes = 0;
    }

        pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&mut self, key: &str) -> Option<Arc<Vec<u8>>> {
        match self.entries.get(key) {
            Some(entry) => {
                self.hits += 1;
                Some(Arc::clone(&entry.output))
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Вставляет выход; при превышении бюджета вытесняет старейшие записи,
    /// включая (при необходимости) саму новую — результат больше кэша полезен
    /// текущему вызову, но кэшировать его бессмысленно.
    pub fn put(&mut self, key: String, output: Arc<Vec<u8>>) {
        let size = output.len();
        if size > self.max_bytes {
            return;
        }
        while self.used_bytes + size > self.max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.used_bytes = self.used_bytes.saturating_sub(evicted.output.len());
            }
        }
        if self.entries.insert(key.clone(), CacheEntry { output }).is_none() {
            self.used_bytes += size;
            self.order.push_back(key);
        }
    }
}

/// Дефолтный бюджет кэша выходов: 256 МБ суммарно на промежуточные результаты.
const DEFAULT_OUTPUT_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// Вместимость реестра активных отмен; одновременных запусков в MVP единицы,
/// лимит защищает от утечки карт при аномальном фронте.
const MAX_ACTIVE_CANCELLATIONS: usize = 64;

pub struct AppState {
    pub registry: TransformRegistry,
    pub sources: RwLock<SourceStore>,
    pub graph: RwLock<Graph>,
    /// Time-Travel история (FR-4): пишется при каждом успешном `run_node`,
    /// читается `list_snapshots`. Отдельный лок — история и источник байтов
    /// имеют разные времена жизни и никогда не блокируются вложенно.
    pub history: RwLock<History>,
    /// Content-addressed кэш выходов (memoization планировщика).
    pub cache: Mutex<OutputCache>,
    /// Активные кооперативные отмены по запрошенному nodeId. Mutex (не RwLock):
    /// операции короткие insert/remove/take.
    pub cancellations: Mutex<HashMap<NodeId, CancellationToken>>,
}

impl AppState {
    pub fn new(registry: TransformRegistry) -> Self {
        Self::with_cache_budget(registry, DEFAULT_OUTPUT_CACHE_BYTES)
    }

    /// Тестовый конструктор с переопределением бюджета кэша.
    pub fn with_cache_budget(registry: TransformRegistry, cache_max_bytes: usize) -> Self {
        Self {
            registry,
            sources: RwLock::new(SourceStore::default()),
            graph: RwLock::new(Graph::new()),
            history: RwLock::new(History::default()),
            cache: Mutex::new(OutputCache::new(cache_max_bytes)),
            cancellations: Mutex::new(HashMap::new()),
        }
    }

    /// Регистрирует токен отмены; `false` = лимит активных запусков исчерпан.
    pub fn register_cancellation(&self, node_id: NodeId, token: CancellationToken) -> bool {
        let mut map = self.cancellations.lock();
        if map.len() >= MAX_ACTIVE_CANCELLATIONS {
            return false;
        }
        map.insert(node_id, token);
        true
    }

    /// Снимает и возвращает токен по завершении выполнения (успех/ошибка).
    pub fn take_cancellation(&self, node_id: &NodeId) -> Option<CancellationToken> {
        self.cancellations.lock().remove(node_id)
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
        let cache = state.cache.lock();
        assert_eq!((cache.hits, cache.misses), (0, 0));
        drop(cache);
        assert!(state.cancellations.lock().is_empty());
    }

    #[test]
    fn output_cache_hit_miss_and_eviction() {
        let mut cache = OutputCache::new(100);

        assert!(cache.get("k1").is_none());
        assert_eq!(cache.misses, 1);

        cache.put("k1".into(), Arc::new(vec![0; 60]));
        cache.put("k2".into(), Arc::new(vec![0; 30]));
        let hit = cache.get("k1").expect("fresh entry must hit");
        assert_eq!(hit.len(), 60);
        assert_eq!(cache.hits, 1);

        // Вставка 50 байт при бюджете 100 вытесняет САМЫЙ СТАРЫЙ k1 (60):
        // 90+50 > 100 → evict k1 → used = 30(k2)+50(k3) = 80 <= 100.
        cache.put("k3".into(), Arc::new(vec![0; 50]));
        assert!(
            cache.get("k1").is_none(),
            "oldest-inserted entry must be evicted first"
        );
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k3").is_some());

        // Результат больше бюджета не кэшируется вовсе.
        cache.put("huge".into(), Arc::new(vec![0; 500]));
        assert!(cache.get("huge").is_none());
    }

    #[test]
    fn write_region_overwrites_in_memory_bounds_checked() {
        use super::WriteRegionError;
        let mut store = SourceStore::default();
        let h = store.insert(SourceEntry::InMemory(b"HELLO".to_vec()));

        let new_size = store.write_region(&h, 1, b"EY!").expect("in-bounds patch");
        assert_eq!(new_size, 5);
        // HELLO → HEY!O: три байта с позиции 1 заменены, хвост не тронут.
        assert_eq!(store.get(&h).unwrap().as_bytes(), b"HEY!O");

        // Выход за границы отвергается, содержимое не меняется.
        let err = store.write_region(&h, 3, b"TOOLONG").unwrap_err();
        assert_eq!(
            err,
            WriteRegionError::OutOfBounds { size: 5, required_end: 10 }
        );
        assert_eq!(store.get(&h).unwrap().as_bytes(), b"HEY!O");

        // Нулевая длина по границе — допустимый no-op.
        store.write_region(&h, 5, b"").unwrap();
        assert_eq!(store.get(&h).unwrap().as_bytes(), b"HEY!O");

        // Неизвестный handle.
        assert_eq!(
            store.write_region(&uuid::Uuid::new_v4(), 0, b"x"),
            Err(WriteRegionError::UnknownHandle)
        );
    }

    #[test]
    fn write_region_error_variants_are_distinguishable() {
        use super::WriteRegionError;
        assert_ne!(
            WriteRegionError::ReadOnlyMapped,
            WriteRegionError::UnknownHandle
        );
    }

    #[test]
    fn output_cache_clear_invalidates_everything() {
        let mut cache = OutputCache::new(100);
        cache.put("k".into(), Arc::new(vec![0; 10]));
        assert!(cache.get("k").is_some());

        cache.clear();
        assert!(cache.get("k").is_none(), "после clear старых hit'ов нет");
        assert_eq!(cache.entries_len(), 0);
    }

    #[test]
    fn cancellation_register_take_roundtrip() {
        let state = AppState::new(hexforge_ops::build_registry());
        let id: NodeId = Uuid::new_v4();
        let token: CancellationToken = Arc::new(AtomicBool::new(false));

        assert!(state.register_cancellation(id, token.clone()));
        assert!(Arc::ptr_eq(
            &state.take_cancellation(&id).expect("token registered"),
            &token
        ));
        assert!(state.take_cancellation(&id).is_none(), "take is one-shot");
    }
}
