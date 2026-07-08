//! Реализация Tauri commands. Каждая структура запроса/ответа здесь —
//! зеркало соответствующего типа в `src/lib/ipc-contract.ts`
//! (см. `05-IPC-CONTRACT.md`). `#[serde(rename_all = "camelCase")]`
//! обеспечивает совпадение имён полей с TS без ручного маппинга.

use crate::error::{HexForgeError, HexForgeResult};
use crate::state::{AppState, SourceEntry};
use base64::{engine::general_purpose, Engine as _};
use hexforge_core::graph::{Graph, NodeId, OperationNode};
use hexforge_core::transform::{ExecutionContext, NullExecutionContext};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tauri::State;
use uuid::Uuid;

/// Верификационная команда моста Rust<->React для Этапа 2 ("greet").
/// Держим её постоянно как smoke-test канала IPC, а не только как временный
/// шаг: `list_operations` — первая "настоящая" команда, `greet` — самый
/// дешёвый способ проверить, что мост вообще жив (напр. в E2E-тестах).
#[tauri::command]
pub fn greet(name: String) -> String {
    format!("HexForge core is online. Hello, {name}.")
}

// ---------- Реестр операций ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDescriptor {
    pub id: String,
    pub version: String,
    pub display_name: String,
    pub category: String,
    pub params_schema: serde_json::Value,
    pub capabilities: hexforge_core::TransformCapabilities,
}

#[tauri::command]
pub fn list_operations(state: State<AppState>) -> Vec<OperationDescriptor> {
    state
        .registry
        .iter()
        .map(|t| OperationDescriptor {
            id: t.id().to_string(),
            version: t.version().to_string(),
            display_name: t.display_name().to_string(),
            category: t.category().to_string(),
            params_schema: t.params_schema(),
            capabilities: t.capabilities(),
        })
        .collect()
}

// ---------- Источники данных ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFileRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFileResponse {
    pub handle: String,
    pub size_bytes: u64,
    pub detected_mime: Option<String>,
}

#[tauri::command]
pub fn open_file(
    req: OpenFileRequest,
    state: State<AppState>,
) -> HexForgeResult<OpenFileResponse> {
    let file = std::fs::File::open(&req.path).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot open '{}': {e}", req.path))
    })?;

    // SAFETY: memmap2::Mmap::map is unsafe because the OS gives no guarantee
    // the backing file won't be truncated/modified by another process while
    // mapped, which can turn a read into a SIGBUS (Unix) or produce garbage
    // bytes rather than UB in the Rust-safety sense. We accept this risk
    // explicitly for the desktop-tool use case (single local user, files
    // typically not concurrently mutated by another writer) rather than
    // paying for a full read into an owned buffer, which would defeat the
    // NFR-2 zero-copy requirement for 32GB inputs. If this ever needs to be
    // hardened, wrap reads in a SIGBUS handler or fall back to buffered
    // chunked reads when the source file is detected as still open for
    // writing elsewhere.
    let mmap = unsafe { memmap2::Mmap::map(&file) }
        .map_err(|e| HexForgeError::internal(format!("mmap failed: {e}")))?;

    let size_bytes = mmap.len() as u64;
    let detected_mime = detect_mime(&mmap);

    let handle = state.sources.write().insert(SourceEntry::Mapped(mmap));

    Ok(OpenFileResponse {
        handle: handle.to_string(),
        size_bytes,
        detected_mime,
    })
}

/// Минимальная магик-байт детекция для MVP; полноценный "Magic Wand" (FR-3.9)
/// — отдельный модуль post-MVP, здесь — только самые частые контейнеры.
fn detect_mime(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png".into()),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg".into()),
        [b'P', b'K', 0x03, 0x04, ..] => Some("application/zip".into()),
        [0x1F, 0x8B, ..] => Some("application/gzip".into()),
        [b'M', b'Z', ..] => Some("application/x-msdownload".into()),
        [0x7F, b'E', b'L', b'F', ..] => Some("application/x-elf".into()),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLiteralSourceRequest {
    pub utf8: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLiteralSourceResponse {
    pub handle: String,
    pub size_bytes: u64,
}

/// Контракт (`05-IPC-CONTRACT.md`, §2, `CreateLiteralSourceRequest`) обещает
/// "лимит 16МБ на этом пути" — до этого патча код принимал литерал любого
/// размера, т.е. реальное поведение расходилось с задокументированным
/// контрактом. Без этой проверки создание источника через большой
/// вставленный текст могло обойти планировщик стриминга целиком.
const MAX_LITERAL_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[tauri::command]
pub fn create_literal_source(
    req: CreateLiteralSourceRequest,
    state: State<AppState>,
) -> HexForgeResult<CreateLiteralSourceResponse> {
    let bytes = req.utf8.into_bytes();
    if bytes.len() > MAX_LITERAL_SOURCE_BYTES {
        return Err(HexForgeError::invalid_parameter(
            "utf8",
            format!(
                "literal source exceeds {}MB limit ({} bytes given); use open_file for larger inputs",
                MAX_LITERAL_SOURCE_BYTES / (1024 * 1024),
                bytes.len()
            ),
        ));
    }
    let size_bytes = bytes.len() as u64;
    let handle = state.sources.write().insert(SourceEntry::InMemory(bytes));
    Ok(CreateLiteralSourceResponse {
        handle: handle.to_string(),
        size_bytes,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBytesRequest {
    pub handle: String,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBytesResponse {
    pub base64_chunk: String,
    pub actual_length: u64,
}

/// Верхняя граница на один запрос превью — гарантируется сервером
/// (не клиентом), см. `05-IPC-CONTRACT.md` §2, PreviewBytesRequest.
const MAX_PREVIEW_LENGTH: u64 = 1024 * 1024;

#[tauri::command]
pub fn preview_bytes(
    req: PreviewBytesRequest,
    state: State<AppState>,
) -> HexForgeResult<PreviewBytesResponse> {
    let handle = parse_handle(&req.handle)?;
    let sources = state.sources.read();
    let entry = sources
        .get(&handle)
        .ok_or_else(|| HexForgeError::invalid_input(format!("unknown source handle: {}", req.handle)))?;

    let bytes = entry.as_bytes();
    let start = (req.offset as usize).min(bytes.len());
    let requested_len = req.length.min(MAX_PREVIEW_LENGTH) as usize;
    // saturating_add: `offset`/`length` приходят из фронтенда как u64 и в
    // принципе могут быть сколь угодно большими (напр. UI-баг передал
    // offset близко к u64::MAX) — обычное сложение здесь могло бы
    // переполниться на всех платформах, кроме тех, где usize == u64 и
    // значения малы; saturating_add убирает саму возможность паники/UB
    // независимо от входных значений, не полагаясь на то, что вызывающая
    // сторона всегда пришлёт разумные числа.
    let end = start.saturating_add(requested_len).min(bytes.len());
    let slice = &bytes[start..end];

    Ok(PreviewBytesResponse {
        base64_chunk: general_purpose::STANDARD.encode(slice),
        actual_length: slice.len() as u64,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSourceRequest {
    pub handle: String,
}

#[tauri::command]
pub fn release_source(req: ReleaseSourceRequest, state: State<AppState>) -> bool {
    match parse_handle(&req.handle) {
        Ok(handle) => state.sources.write().release(&handle),
        Err(_) => false,
    }
}

fn parse_handle(raw: &str) -> HexForgeResult<Uuid> {
    Uuid::parse_str(raw).map_err(|_| HexForgeError::invalid_input(format!("'{raw}' is not a valid source handle")))
}

// ---------- Граф ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationNodeDto {
    pub id: String,
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    pub inputs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDto {
    pub nodes: std::collections::HashMap<String, OperationNodeDto>,
}

impl TryFrom<GraphDto> for Graph {
    type Error = HexForgeError;

    fn try_from(dto: GraphDto) -> Result<Self, Self::Error> {
        let mut graph = Graph::new();
        for (_id, node) in dto.nodes {
            let id: NodeId = parse_handle(&node.id)?;
            let inputs = node
                .inputs
                .iter()
                .map(|s| parse_handle(s))
                .collect::<Result<Vec<_>, _>>()?;
            graph.insert_node(OperationNode {
                id,
                operation_id: node.operation_id,
                operation_version: node.operation_version,
                params: node.params,
                inputs,
            });
        }
        Ok(graph)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGraphRequest {
    pub graph: GraphDto,
}

#[tauri::command]
pub fn set_graph(req: SetGraphRequest, state: State<AppState>) -> HexForgeResult<()> {
    let graph: Graph = req.graph.try_into()?;
    // Валидация DAG до принятия графа — узел с циклом никогда не попадёт
    // в состояние приложения (FR "граф всегда ациклический").
    graph.topo_order().map_err(HexForgeError::from)?;
    *state.graph.write() = graph;
    Ok(())
}

// ---------- Выполнение узла ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunNodeRequest {
    pub node_id: String,
    pub preview_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunNodeResponse {
    pub output_handle: String,
    pub output_size_bytes: u64,
    pub duration_ms: u64,
}

/// MVP-исполнитель одного узла: наивная рекурсия по графу без мемоизации
/// промежуточных результатов и без чанкового стриминга — это заглушка,
/// закрывающая контракт `run_node` для проверки моста и однопутевых
/// (non-merge) рецептов. Полноценный планировщик с topo-order execution,
/// кэшированием по `Snapshot::reproducibility_key()` и chunked streaming
/// живёт в `hexforge-stream` (см. `04-RUST-CORE-ARCHITECTURE.md`, §6) и
/// подключается сюда без изменения этого IPC-контракта.
#[tauri::command]
pub fn run_node(req: RunNodeRequest, state: State<AppState>) -> HexForgeResult<RunNodeResponse> {
    let node_id = parse_handle(&req.node_id)?;
    let started = Instant::now();
    let output = resolve_node_output(&node_id, &state)?;
    let output_size_bytes = output.len() as u64;

    let output_handle = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(output));

    Ok(RunNodeResponse {
        output_handle: output_handle.to_string(),
        output_size_bytes,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn resolve_node_output(node_id: &NodeId, state: &State<AppState>) -> HexForgeResult<Vec<u8>> {
    let node = {
        let graph = state.graph.read();
        graph.nodes.get(node_id).cloned().ok_or_else(|| {
            HexForgeError::internal_for_node(node_id, format!("node {node_id} not found in current graph"))
        })?
    };

    let input_bytes: Vec<u8> = match node.inputs.len() {
        0 => {
            // Корневой узел — байты берутся из SourceStore по хэндлу,
            // переданному в params.source_handle (см. комментарий в
            // `05-IPC-CONTRACT.md` о конвенции корневых узлов).
            let handle_str = node
                .params
                .get("sourceHandle")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    HexForgeError::invalid_parameter("sourceHandle", "root node requires params.sourceHandle")
                })?;
            let handle = parse_handle(handle_str)?;
            let sources = state.sources.read();
            let entry = sources
                .get(&handle)
                .ok_or_else(|| HexForgeError::internal_for_node(node_id, format!("unknown source handle: {handle_str}")))?;
            // NOTE (задокументированный tech debt, см. docs/07 §"Известные
            // ограничения"): `.to_vec()` копирует весь корневой буфер на
            // каждый вызов, что противоречит NFR-2 (zero-copy). Осознанно
            // не исправлено в этом патче: убрать копию можно только вместе
            // с редизайном сигнатуры под `Cow<[u8]>`, привязанный к времени
            // жизни `RwLockReadGuard`, а этот guard приходится держать
            // одновременно с рекурсивным вызовом ниже по графу, который сам
            // берёт локи на `state.sources`/`state.graph` — наивная попытка
            // вернуть заимствование отсюда либо не скомпилируется, либо
            // (если протолкнуть через unsafe) создаст риск дедлока при
            // конкурентном доступе. Правильное решение — планировщик
            // `hexforge-stream`, а не точечный патч этой функции.
            entry.as_bytes().to_vec()
        }
        1 => resolve_node_output(&node.inputs[0], state)?,
        _ => {
            return Err(HexForgeError::internal_for_node(
                node_id,
                "multi-input merge nodes are not yet implemented in the MVP command layer; \
                 requires the hexforge-stream N-ary scheduler",
            ))
        }
    };

    let transform = state
        .registry
        .get(&node.operation_id)
        .ok_or_else(|| HexForgeError::internal_for_node(node_id, format!("unknown operation: {}", node.operation_id)))?;

    if transform.version() != node.operation_version {
        return Err(HexForgeError::internal_for_node(
            node_id,
            format!(
                "operation '{}' version mismatch: node expects {}, registry has {} \
                 (reproducibility guarantee violated, see FR-4.2)",
                node.operation_id,
                node.operation_version,
                transform.version()
            ),
        ));
    }

    let ctx: &dyn ExecutionContext = &NullExecutionContext;
    let result = transform
        .apply(std::borrow::Cow::Owned(input_bytes), &node.params, ctx)
        .map_err(HexForgeError::from)?;

    Ok(result.into_owned())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNodeRequest {
    pub node_id: String,
}

/// MVP: выполнение узлов синхронно и коротко в рамках Этапа 2, поэтому
/// настоящая кооперативная отмена ещё не подключена — она приходит вместе
/// с async-планировщиком `hexforge-stream`. Команда уже присутствует в
/// контракте, чтобы фронтенд не менялся, когда появится реальная отмена.
#[tauri::command]
pub fn cancel_node(_req: CancelNodeRequest) -> bool {
    false
}

// ---------- History (заглушки контракта на Этапе 2) ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDto {
    pub id: String,
    pub parent: Option<String>,
    pub node_id: String,
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    pub input_content_hash: String,
    pub output_content_hash: Option<String>,
}

#[tauri::command]
pub fn list_snapshots() -> Vec<SnapshotDto> {
    // История как state-DAG подключается вместе с write-through в
    // `hexforge-core::History` при каждом `run_node` — намеренно не
    // реализовано в этом срезе Этапа 2, чтобы не расширять IPC-контракт
    // до его ревью; сигнатура команды уже финальна.
    Vec::new()
}

// ---------- Плагины (заглушки контракта на Этапе 2) ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub signature_valid: bool,
    pub requested_capabilities: Vec<String>,
    pub granted_capabilities: Vec<String>,
}

#[tauri::command]
pub fn list_plugins() -> Vec<PluginManifestDto> {
    // hexforge-plugin-host (Wasmtime runtime) — отдельный крейт, ещё не
    // реализован в этом срезе; команда возвращает пустой список, а не
    // ошибку, чтобы UI плагин-менеджера уже сейчас рендерил пустое
    // состояние корректно.
    Vec::new()
}
