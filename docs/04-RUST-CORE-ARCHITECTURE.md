# HexForge — Rust Core Architecture & Crate Design

## 1. Cargo Workspace

```
hexforge/
├── Cargo.toml                     # [workspace], resolver = "2"
├── crates/
│   ├── hexforge-core/             # Domain model: Transform trait, Node, Graph, Snapshot. Zero I/O, zero UI knowledge.
│   ├── hexforge-ops/               # Конкретные операции (impl Transform), сгруппированные по категориям через features
│   ├── hexforge-stream/            # Chunked I/O, mmap, zero-copy slicing, backpressure
│   ├── hexforge-engine/            # Исполнитель: AppState, планировщик (execute_chain/replay_snapshot), OutputCache, отмена. Без Tauri.
│   ├── hexforge-plugin-host/       # Wasmtime runtime, capability sandbox, plugin manifest/signature verification
│   └── hexforge-cli/               # Тонкий бинарь: recipe runner без GUI, использует те же крейты, что и src-tauri
└── src-tauri/                      # Tauri shell: commands.rs — единственное место, знающее и про Tauri, и про движок
```

Правило зависимостей (однонаправленное, без циклов):

```
src-tauri  ──depends on──▶ hexforge-core, hexforge-ops, hexforge-stream, hexforge-plugin-host
hexforge-cli ──depends on──▶ hexforge-core, hexforge-ops, hexforge-stream
hexforge-ops ──depends on──▶ hexforge-core
hexforge-plugin-host ──depends on──▶ hexforge-core
hexforge-stream ──depends on──▶ (нет зависимости от core: чистый I/O примитив)
hexforge-core ──depends on──▶ (нет внутренних зависимостей)
```

`hexforge-core` не знает о Tauri, Wasmtime, файловой системе — это гарантирует,
что ядро тестируется как чистая библиотека и переиспользуется в CLI без
дублирования.

## 2. Модель ноды и трейт `Transform`

```rust
// crates/hexforge-core/src/transform.rs

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Единица данных, которой оперирует Transform. Может быть zero-copy view
/// над источником (mmap/буфер), либо владеемым результатом.
pub type ByteView<'a> = Cow<'a, [u8]>;

/// Декларация возможностей операции — используется планировщиком стриминга
/// и UI (для оценки памяти/предупреждений, FR-5.3) без выполнения самой операции.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformCapabilities {
    /// Одинаковый вход + одинаковые параметры => гарантированно одинаковый выход.
    pub deterministic: bool,
    /// Операция умеет обрабатывать вход по чанкам, не требуя полного буфера в памяти.
    pub streamable: bool,
    /// Верхняя граница памяти относительно размера входа, для UI-предупреждений.
    pub memory_cost: MemoryCost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryCost {
    /// O(1) относительно размера входа (напр. hashing с потоковым API).
    Constant,
    /// O(chunk_size) — не зависит от полного размера входа.
    PerChunk,
    /// O(n) — вся операция требует полный буфер в памяти (напр. большинство block-cipher режимов без потокового API).
    FullBuffer,
}

/// Ошибка выполнения операции. Единая для всех Transform-реализаций,
/// чтобы UI мог унифицированно рендерить диагностику.
#[derive(Debug, thiserror::Error, Serialize)]
pub enum TransformError {
    #[error("invalid parameter '{field}': {reason}")]
    InvalidParameter { field: String, reason: String },
    #[error("input is not valid for this operation: {reason}")]
    InvalidInput { reason: String },
    #[error("operation exceeded memory budget: {limit_mb}MB")]
    MemoryBudgetExceeded { limit_mb: u64 },
    #[error("internal error: {0}")]
    Internal(String),
}

/// Контекст выполнения — передаётся планировщиком, инкапсулирует
/// возможность кооперативной отмены и репортинг прогресса,
/// без завязки Transform-реализаций на Tokio/Tauri напрямую.
pub trait ExecutionContext: Send + Sync {
    fn report_progress(&self, bytes_processed: u64, bytes_total: Option<u64>);
    fn is_cancelled(&self) -> bool;
}

/// Центральный трейт — единственный контракт, который должно реализовать
/// новое преобразование данных, встроенное или из WASM-плагина.
pub trait Transform: Send + Sync {
    /// Стабильный идентификатор операции, напр. "encoding.base64.decode".
    fn id(&self) -> &'static str;

    /// Semver-версия реализации — фиксируется в снапшотах для воспроизводимости (FR-4.2).
    fn version(&self) -> &'static str;

    /// JSON Schema параметров — фронтенд рендерит форму автоматически (FR-3.2).
    fn params_schema(&self) -> serde_json::Value;

    fn capabilities(&self) -> TransformCapabilities;

    /// Разовое (non-streaming) выполнение над полным буфером.
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError>;

    /// Потоковое выполнение — вызывается планировщиком чанк за чанком
    /// для операций с `capabilities().streamable == true`.
    /// `state` — per-node состояние, принадлежащее планировщику (`Box<dyn Any>`):
    /// операция при первом вызове засеивает свой конкретный тип и далее
    /// работает через downcast_mut.
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        let _ = (chunk, is_last, state, params, ctx);
        Err(TransformError::Internal(
            "apply_chunk not implemented; capabilities().streamable must be false".into(),
        ))
    }
}

/// Контракт N-арных операций слияния (PRD FR-1.2/FR-1.4). Семантика слияния
/// принадлежит операции: узел графа с N>1 входами исполним только если его
/// операция реализует этот трейт (реестр хранит их во второй карте).
pub trait MergeTransform: Transform {
    fn apply_merge<'a>(
        &self,
        inputs: Vec<ByteView<'a>>,
        params: &serde_json::Value,
        ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError>;
}

/// Валидация параметров до выполнения — отдельно от `apply`, чтобы UI мог
/// подсвечивать ошибки формы без запуска операции (мгновенная обратная связь, NFR-1).
pub trait Validate {
    fn validate(&self, params: &serde_json::Value) -> Result<(), Vec<TransformError>>;
}
```

## 3. Модель графа и узла

```rust
// crates/hexforge-core/src/graph.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type NodeId = Uuid;
pub type SourceHandle = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationNode {
    pub id: NodeId,
    pub operation_id: String,      // ссылается в реестр Transform-реализаций
    pub operation_version: String,
    pub params: serde_json::Value,
    pub inputs: Vec<NodeId>,       // 0 входов = источник данных (файл/литерал)
}

/// N-арный граф: допускает несколько inputs (merge) и несколько исходящих
/// рёбер из одного узла (fork) — прямой ответ на FR-1.3/FR-1.4.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Graph {
    pub nodes: std::collections::HashMap<NodeId, OperationNode>,
}

impl Graph {
    /// Топологическая сортировка с детекцией циклов — граф обязан быть DAG.
    pub fn topo_order(&self) -> Result<Vec<NodeId>, GraphError> {
        // Kahn's algorithm; O(V+E). Реализация опущена в архитектурном документе,
        // присутствует полностью в src crate.
        unimplemented!()
    }

    /// Инвалидация: при изменении параметров узла помечает "stale" только
    /// достижимые от него по исходящим рёбрам узлы (FR-1.6) —
    /// не пересчитывает весь граф.
    pub fn downstream_of(&self, node: NodeId) -> Vec<NodeId> {
        unimplemented!()
    }
}

#[derive(Debug, thiserror::Error, Serialize)]
pub enum GraphError {
    #[error("cycle detected in graph")]
    CycleDetected,
    #[error("node {0} references unknown input")]
    DanglingInput(NodeId),
}
```

## 4. Time-Travel Snapshot (history как DAG состояний)

```rust
// crates/hexforge-core/src/history.rs

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SnapshotId = Uuid;

/// Снапшот — не diff и не полный дамп по умолчанию, а воспроизводимая
/// декларация: "если взять source_hash и применить operation@version с
/// params, получится этот результат". Байты результата кэшируются отдельно
/// (content-addressed store) и могут быть вытеснены под memory pressure —
/// воспроизводимость не теряется, снапшот пересчитывается лениво (FR-4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub parent: Option<SnapshotId>,
    pub node_id: crate::graph::NodeId,
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    pub input_content_hash: blake3::Hash,
    pub output_content_hash: Option<blake3::Hash>, // None пока не вычислен/вытеснен
}
```

## 5. Реестр операций (`hexforge-ops`)

`hexforge-ops` не содержит ни одного `match` по имени операции в горячем пути —
регистрация через `inventory` (compile-time collection) избегает
центрального файла-бутылочного горлышка при росте числа операций:

```rust
// crates/hexforge-ops/src/encoding/base64.rs
use hexforge_core::transform::*;

pub struct Base64Decode;

impl Transform for Base64Decode {
    fn id(&self) -> &'static str { "encoding.base64.decode" }
    fn version(&self) -> &'static str { "1.0.0" }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "alphabet": { "type": "string", "enum": ["standard", "url_safe", "custom"], "default": "standard" },
                "custom_alphabet": { "type": "string", "maxLength": 64 }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: true, memory_cost: MemoryCost::PerChunk }
    }
    fn apply<'a>(&self, input: ByteView<'a>, params: &serde_json::Value, _ctx: &dyn ExecutionContext)
        -> Result<ByteView<'a>, TransformError>
    {
        // ... реализация через base64 crate, маппинг ошибок в TransformError::InvalidInput
        unimplemented!()
    }
}

inventory::submit! { &Base64Decode as &dyn Transform }
```

Реестр на старте приложения собирает все `inventory::submit!` в
`TransformRegistry: HashMap<&'static str, &'static dyn Transform>`,
который единственный раз строится в `src-tauri` при инициализации и
передаётся в Tauri `State`.

## 6. Стриминг и планировщик (`hexforge-stream`)

> **Статус MVP (фактическая реализация, обновлено в срезе hexforge-stream):**
> - `hexforge-stream` — чистые чанк-примитивы без знания о домене
>   (`chunk_ranges`, `DEFAULT_CHUNK_SIZE_BYTES` = 1 МиБ), правило зависимостей
>   из §1 соблюдено буквально.
> - Планировщик графа живёт в крейте `hexforge-engine` вместе с
>   `AppState`/кэшем/отменой: ему нужен домен, а переиспользование одним и тем
>   же исполнителем в GUI (`src-tauri`) и CLI (FR-7.3) требует крейта без
>   Tauri-зависимостей. Прогресс отдаётся хосту через callback
>   (`ProgressSink`), а не через `tauri::AppHandle`.
> - Chunked-исполнение: streamable-узлы исполняются чанками `apply_chunk`
>   над zero-copy срезами входа; per-node состояние — `Box<dyn Any>`,
>   засеивается операцией при первом чанке. Cross-node pipelining,
>   bounded backpressure (mpsc) и 64 МБ-чанки FR-5.2 — следующий этап.
> - Memoization: content-addressed LRU-кэш выходов по
>   `reproducibility_key(op@ver :: input_hash :: params)` (см. §4),
>   значения `Arc<Vec<u8>>`, бюджет по байтам (дефолт 256 МБ).
> - Кооперативная отмена: токен на запрошенный узел, чекпоинты между узлами
>   и между чанками streamable-операций; ошибка `Cancelled`.
> - Merge: N-арные узлы исполняются через `MergeTransform::apply_merge`
>   (PRD FR-1.2/FR-1.4); первая операция — `streaming.concat`.

- Источник файла открывается через `memmap2` для файлов на диске (zero-copy),
  и через ручное чтение чанками по 64 МБ (`DEFAULT_CHUNK_SIZE`) там, где mmap
  недоступен (напр. pipe/stdin).
- Планировщик исполняет топологически отсортированный граф: для узлов с
  `streamable == true` пробрасывает `ChunkedExecutor`, который держит per-node
  состояние (`Box<dyn Any>`) между вызовами `apply_chunk` (напр. переносимый
  остаток блока для block-cipher выравнивания).
- Для `streamable == false` узлов планировщик обязан аккумулировать чанки в
  единый буфер **в отдельном blocking-треде** (`tokio::task::spawn_blocking`),
  чтобы не блокировать async-рантайм, и обязан заранее оценить
  `memory_cost` относительно оставшегося системного лимита, отдав
  предупреждение через progress-канал в UI (FR-5.3) до фактического
  накопления буфера.
- Backpressure: канал между чанк-продюсером и исполнителем — bounded
  `tokio::sync::mpsc` с ёмкостью 4 чанка (256МБ верхняя граница буферизации
  на узел), что ограничивает worst-case память графа с N параллельных веток
  величиной `N × 256MB`, а не размером файла.

## 7. Плагины (`hexforge-plugin-host`)

- Рантайм: `wasmtime`, компонентная модель (WIT) — плагин экспортирует
  `transform` интерфейс, изоморфный Rust-трейту `Transform` выше.
- Изоляция: каждый плагин — отдельный `wasmtime::Store` с fuel-лимитом
  (кооперативная защита от infinite loop) и линейной памятью, ограниченной
  манифестом (`max_memory_mb`, дефолт 256МБ).
- Подпись: манифест плагина подписывается Ed25519; публичный ключ автора
  фиксируется при первой установке (TOFU) либо сверяется с встроенным
  реестром доверенных издателей. Несовпадение подписи — hard fail, не warning.
- Capability grants: плагин по умолчанию не имеет доступа к файловой системе
  и сети; если манифест запрашивает капабилити, Tauri-слой показывает
  `CapabilityGrantDialog` (см. `03-INFORMATION-ARCHITECTURE`) и хранит grant
  per-plugin per-capability, отзываемый пользователем в любой момент.

## 8. Почему не async-trait / dyn-friendly дизайн

`Transform::apply` — синхронная функция, исполняемая планировщиком в
thread pool (`rayon` для CPU-bound операций, никогда не в async executor
напрямую). Это осознанное решение: подавляющее большинство операций
(кодирование, хеширование, крипто) — чистый CPU-bound код без I/O внутри
самой трансформации, и оборачивание каждой в `async fn` только добавляет
overhead аллокации Future без выгоды. Асинхронность нужна на границе
"UI ждёт результат", а не внутри самой операции — она обеспечивается
на уровне Tauri command (`spawn_blocking` + `oneshot` канал результата),
не на уровне трейта.
