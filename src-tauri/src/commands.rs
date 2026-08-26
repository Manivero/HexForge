//! Реализация Tauri commands. Каждая структура запроса/ответа здесь —
//! зеркало соответствующего типа в `src/lib/ipc-contract.ts`
//! (см. `05-IPC-CONTRACT.md`). `#[serde(rename_all = "camelCase")]`
//! обеспечивает совпадение имён полей с TS без ручного маппинга.

use hexforge_engine::graph_dto::GraphDto;
use hexforge_engine::error::{HexForgeError, HexForgeResult};
use hexforge_engine::scheduler;
use hexforge_engine::state::{AppState, SourceEntry, WriteRegionError};
use base64::{engine::general_purpose, Engine as _};
use hexforge_core::graph::Graph;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, State};
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

/// Детерминированный порядок операций для UI (⌘K): категория → имя → id.
/// Итерация HashMap реестра неупорядочена — без сортировки список менялся бы
/// между запусками приложения.
fn sort_for_palette(v: &mut [OperationDescriptor]) {
    v.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.display_name.cmp(&b.display_name))
            .then_with(|| a.id.cmp(&b.id))
    });
}
#[tauri::command]
pub fn list_operations(state: State<Arc<AppState>>) -> Vec<OperationDescriptor> {
    let mut descriptors: Vec<OperationDescriptor> = state
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
        .collect();
    sort_for_palette(&mut descriptors);
    descriptors

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
    state: State<Arc<AppState>>,
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
    state: State<Arc<AppState>>,
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
    state: State<Arc<AppState>>,
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
pub fn release_source(req: ReleaseSourceRequest, state: State<Arc<AppState>>) -> bool {
    match parse_handle(&req.handle) {
        Ok(handle) => state.sources.write().release(&handle),
        Err(_) => false,
    }
}

fn parse_handle(raw: &str) -> HexForgeResult<Uuid> {
    Uuid::parse_str(raw).map_err(|_| HexForgeError::invalid_input(format!("'{raw}' is not a valid source handle")))
}

// ---------- Patch source (FR Hex Editor) ----------

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSourceRequest {
    pub handle: String,
    /// Смещение первого перезаписываемого байта.
    pub offset: u64,
    /// Байты для перезаписи (base64). Только в границах текущего размера.
    pub bytes_base64: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSourceResponse {
    pub new_size_bytes: u64,
}

/// Точечная перезапись региона InMemory-источника (FR Hex Editor).
/// Семантика MVP: без роста и без записи в memory-mapped файлы — обе
/// ситуации возвращают типизированную ошибку. Content-addressed кэш
/// планировщика не инвалидируется явно: ключи по content-hash, патч меняет
/// хэши будущих прогонов естественным образом, старые снапшоты остаются
/// корректными записями прошлого (FR-4.2).
#[tauri::command]
pub fn patch_source(
    req: PatchSourceRequest,
    state: State<Arc<AppState>>,
) -> HexForgeResult<PatchSourceResponse> {
    let handle = parse_handle(&req.handle)?;
    let data = general_purpose::STANDARD
        .decode(req.bytes_base64.as_bytes())
        .map_err(|e| {
            HexForgeError::invalid_parameter("bytesBase64", format!("not valid base64: {e}"))
        })?;
    let offset = usize::try_from(req.offset)
        .map_err(|_| HexForgeError::invalid_parameter("offset", "offset is out of range"))?;

    let mut sources = state.sources.write();
    let new_size = sources.write_region(&handle, offset, &data).map_err(|e| match e {
        WriteRegionError::UnknownHandle => {
            HexForgeError::invalid_input(format!("unknown source handle: {}", req.handle))
        }
        WriteRegionError::OutOfBounds { size, required_end } => HexForgeError::invalid_parameter(
            "bytesBase64",
            format!(
                "patch range [{offset}..{required_end}) exceeds source size {size}; growth is not supported in MVP"
            ),
        ),
        WriteRegionError::ReadOnlyMapped => HexForgeError::invalid_input(
            "source is a memory-mapped file and cannot be patched (read-only MVP)",
        ),
    })?;

    Ok(PatchSourceResponse {
        new_size_bytes: new_size as u64,
    })
}



#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGraphRequest {
    pub graph: GraphDto,
}

/// Контракт docs/05 §3: после валидации DAG бэкенд считает stale-набор
/// (изменённые узлы ∪ их downstream) и эмитит graph://invalidated —
/// фронтенд подсвечивает устаревшие узлы без локальной эвристики.
#[tauri::command]
pub async fn set_graph(
    req: SetGraphRequest,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> HexForgeResult<()> {
    let old_graph = state.graph.read().clone();
    let graph: Graph = req.graph.try_into()?;
    // Валидация DAG до принятия графа — узел с циклом никогда не попадёт
    // в состояние приложения (FR "граф всегда ациклический").
    graph.topo_order().map_err(HexForgeError::from)?;

    let stale = scheduler::compute_invalidated(&old_graph, &graph);
    *state.graph.write() = graph;

    if !stale.is_empty() {
        use tauri::Emitter;
        // Ошибка доставки сознательно игнорируется: нет слушателя — не беда.
        let _ = app.emit(
            "graph://invalidated",
            hexforge_engine::scheduler::GraphInvalidatedEvent { stale_node_ids: stale },
        );
    }
    Ok(())
}

// ---------- Выполнение узла ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunNodeRequest {
    pub node_id: String,
    /// FR-1.6 (`previewOnly=true`: downstream не пересчитывается). MVP-исполнитель
    /// и так никогда не трогает downstream — выполняется только запрошенный узел
    /// и его входная цепочка, — поэтому различение режимов появится вместе с
    /// планировщиком `hexforge-stream`; поле принимается ради совместимости
    /// контракта (`05-IPC-CONTRACT.md`).
    #[allow(dead_code)]
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
pub async fn run_node(
    req: RunNodeRequest,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> HexForgeResult<RunNodeResponse> {
    let node_id = parse_handle(&req.node_id)?;
    let started = Instant::now();
    let exec_state = Arc::clone(state.inner());

    // Кооперативная отмена: токен живёт в AppState до завершения запуска;
    // cancel_node находит его по запрошенному nodeId и выставляет флаг.
    let token: hexforge_engine::state::CancellationToken = Arc::new(AtomicBool::new(false));
    if !exec_state.register_cancellation(node_id, Arc::clone(&token)) {
        return Err(HexForgeError::invalid_input(
            "too many concurrent node executions; cancel a running node first",
        ));
    }

    // Принцип №2 IPC-контракта (`05-IPC-CONTRACT.md` §1): команда, способная
    // выполняться дольше 16ms, обязана быть async и не блокировать рантайм.
    // CPU-bound планировщик уходит в blocking-пул; async-задача только ждёт
    // результат и репортит прогресс через op://progress.
    let task_token = Arc::clone(&token);
    let task_state = Arc::clone(&exec_state);
    let output = tauri::async_runtime::spawn_blocking(move || {
        // Прогресс уходит в WebView; ошибка доставки сознательно игнорируется.
        let on_progress = |event: &hexforge_engine::scheduler::ProgressEvent| {
            let _ = app.emit("op://progress", event);
        };
        scheduler::execute_chain(&task_state, &node_id, &task_token, &on_progress)
    })
    .await
    .map_err(|e| HexForgeError::internal(format!("node execution worker failed: {e}")))??;

    // Гарантированный cleanup реестра отмен (успех или ошибка — токен снят).
    // Если cancel_node уже изъял токен (one-shot), take вернёт None — это норм.
    let _ = exec_state.take_cancellation(&node_id);

    let output_size_bytes = output.len() as u64;
    let output_handle = state
        .sources
        .write()
        .insert(SourceEntry::InMemory((*output).clone()));

    Ok(RunNodeResponse {
        output_handle: output_handle.to_string(),
        output_size_bytes,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNodeRequest {
    pub node_id: String,
}

/// Кооперативная отмена запущенного узла (контракт: `bool` — был ли найден
/// активный запуск). Токен изымается из реестра и выставляется флагом:
/// планировщик замечает это на ближайшем чекпоинте (между узлами или между
/// чанками streamable-операции) и завершается ошибкой `Cancelled`.
/// Повторный cancel того же запуска вернёт `false` — отмена one-shot.
#[tauri::command]
pub fn cancel_node(req: CancelNodeRequest, state: State<Arc<AppState>>) -> bool {
    let Ok(node_id) = parse_handle(&req.node_id) else {
        return false;
    };
    match state.take_cancellation(&node_id) {
        Some(token) => {
            token.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecipeRequest {
    pub graph: GraphDto,
    pub target_path: String,
}

// ---------- Time-Travel (FR-4) ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpToSnapshotRequest {
    pub snapshot_id: String,
}

/// Time-Travel (FR-4.1): лениво пересчитывает выход снапшота из корневого
/// источника через lineage-реплей (scheduler::replay_snapshot), кладёт
/// результат в SourceStore и возвращает стандартный RunNodeResponse
/// (контракт docs/05: "лениво пересчитывает"). Прыжок переносит голову
/// истории на целевой снапшот — последующие запуски ветвятся от этой точки.
#[tauri::command]
pub async fn jump_to_snapshot(
    req: JumpToSnapshotRequest,
    state: State<'_, Arc<AppState>>,
) -> HexForgeResult<RunNodeResponse> {
    let snapshot_id = parse_handle(&req.snapshot_id)?;
    let started = Instant::now();
    let exec_state = Arc::clone(state.inner());
    let history_state = Arc::clone(&exec_state);

    let output = tauri::async_runtime::spawn_blocking(move || {
        scheduler::replay_snapshot(&exec_state, snapshot_id)
    })
    .await
    .map_err(|e| HexForgeError::internal(format!("replay worker failed: {e}")))??;

    // FR-4.1: прыжок переносит голову истории на целевой снапшот.
    history_state.history.write().current = Some(snapshot_id);

    let output_size_bytes = output.len() as u64;
    let output_handle = state
        .sources
        .write()
        .insert(SourceEntry::InMemory((*output).clone()));

    Ok(RunNodeResponse {
        output_handle: output_handle.to_string(),
        output_size_bytes,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

// ---------- Экспорт/импорт рецептов ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecipeRequest {
    pub source_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecipeResponse {
    pub graph: GraphDto,
    /// Операции, которых нет в реестре либо версия которых отличается от
    /// запрошенной рецептом — UI обязан явно показать список (FR-4.2).
    pub missing_operations: Vec<String>,
}

/// Сохраняет граф в JSON (структура `GraphDto` 1:1 с ipc-contract.ts —
/// формат файла является частью публичного контракта). Экспорт строго
/// валидируется: невалидный DAG или операции, отсутствующие в реестре /
/// несовпадающей версии, делают рецепт невоспроизводимым, поэтому такой
/// экспорт отклоняется сразу.
#[tauri::command]
pub fn export_recipe(
    req: ExportRecipeRequest,
    state: State<'_, Arc<AppState>>,
) -> HexForgeResult<()> {
    export_recipe_inner(state.inner(), req)
}

fn export_recipe_inner(state: &AppState, req: ExportRecipeRequest) -> HexForgeResult<()> {
    let graph: Graph = req.graph.clone().try_into()?;
    graph.topo_order().map_err(HexForgeError::from)?;

    let mut missing: Vec<String> = Vec::new();
    for node in graph.nodes.values() {
        let reproducible = state
            .registry
            .get(&node.operation_id)
            .map(|t| t.version() == node.operation_version)
            .unwrap_or(false);
        if !reproducible && !missing.contains(&node.operation_id) {
            missing.push(node.operation_id.clone());
        }
    }
    if !missing.is_empty() {
        return Err(HexForgeError::invalid_input(format!(
            "cannot export recipe: operations missing from registry or version-mismatched: {}",
            missing.join(", ")
        )));
    }

    let json = serde_json::to_string_pretty(&req.graph)
        .map_err(|e| HexForgeError::internal(format!("recipe serialization failed: {e}")))?;
    std::fs::write(&req.target_path, json).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot write '{}': {e}", req.target_path))
    })?;
    Ok(())
}

/// Читает рецепт и возвращает граф + список операций, отсутствующих в
/// текущем реестре (или имеющих другую версию). Импорт НЕ отклоняет граф с
/// missingOperations — это валидный переносимый рецепт; раннюю диагностику
/// даёт список, а жёсткий контроль всё равно сработает в `run_node`
/// (version mismatch, FR-4.2).
#[tauri::command]
pub fn import_recipe(
    req: ImportRecipeRequest,
    state: State<'_, Arc<AppState>>,
) -> HexForgeResult<ImportRecipeResponse> {
    import_recipe_inner(state.inner(), req)
}

fn import_recipe_inner(state: &AppState, req: ImportRecipeRequest) -> HexForgeResult<ImportRecipeResponse> {
    let text = std::fs::read_to_string(&req.source_path).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot read '{}': {e}", req.source_path))
    })?;
    let dto: GraphDto = serde_json::from_str(&text).map_err(|e| {
        HexForgeError::invalid_input(format!(
            "'{}' is not a valid recipe file: {e}",
            req.source_path
        ))
    })?;

    // Конвертация в Graph валидирует UUID'ы, topo_order — ацикличность;
    // сам dto возвращается как есть (формат файла == контракт GraphDto).
    let graph: Graph = dto.clone().try_into()?;
    graph.topo_order().map_err(HexForgeError::from)?;

    let mut missing = std::collections::BTreeSet::new();
    for node in graph.nodes.values() {
        let known = state
            .registry
            .get(&node.operation_id)
            .map(|t| t.version() == node.operation_version)
            .unwrap_or(false);
        if !known {
            missing.insert(node.operation_id.clone());
        }
    }

    Ok(ImportRecipeResponse {
        graph: dto,
        missing_operations: missing.into_iter().collect(),
    })
}

// ---------- History ----------

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

/// Возвращает журнал снапшотов в порядке записи (см. `History::order`).
/// Каждый успешный `run_node` пишет по одному снапшоту на выполненный узел
/// входной цепочки; байты результатов не пересекают границу IPC — только
/// content-hash'и (FR-4.2), сами байты доступны через `preview_bytes`.
#[tauri::command]
pub fn list_snapshots(state: State<Arc<AppState>>) -> Vec<SnapshotDto> {
    list_snapshots_inner(state.inner())
}

fn list_snapshots_inner(state: &AppState) -> Vec<SnapshotDto> {
    let history = state.history.read();
    history
        .ordered_snapshots()
        .iter()
        .map(|s| SnapshotDto {
            id: s.id.to_string(),
            parent: s.parent.map(|p| p.to_string()),
            node_id: s.node_id.to_string(),
            operation_id: s.operation_id.clone(),
            operation_version: s.operation_version.clone(),
            params: s.params.clone(),
            input_content_hash: s.input_content_hash.to_hex().to_string(),
            output_content_hash: s
                .output_content_hash
                .map(|h| h.to_hex().to_string()),
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::graph::{NodeId, OperationNode};
    use hexforge_engine::graph_dto::OperationNodeDto;
    use hexforge_engine::error::HexForgeErrorKind;

    #[test]
    fn detect_mime_known_magic_bytes() {
        assert_eq!(
            detect_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).as_deref(),
            Some("image/png")
        );
        assert_eq!(detect_mime(&[0xFF, 0xD8, 0xFF, 0xE0]).as_deref(), Some("image/jpeg"));
        assert_eq!(detect_mime(b"PK\x03\x04rest").as_deref(), Some("application/zip"));
        assert_eq!(detect_mime(&[0x1F, 0x8B, 0x08, 0x00]).as_deref(), Some("application/gzip"));
        assert_eq!(
            detect_mime(b"MZ\x90\x00\x03").as_deref(),
            Some("application/x-msdownload")
        );
        assert_eq!(
            detect_mime(&[0x7F, b'E', b'L', b'F', 0x02, 0x01]).as_deref(),
            Some("application/x-elf")
        );
    }

    #[test]
    fn detect_mime_unknown_and_short_inputs() {
        assert_eq!(detect_mime(b"plain text").as_deref(), None);
        // Входы короче любой магической последовательности не паникуют.
        assert_eq!(detect_mime(&[]).as_deref(), None);
        assert_eq!(detect_mime(&[0x89]).as_deref(), None);
        assert_eq!(detect_mime(&[0x1F]).as_deref(), None);
    }

    #[test]
    fn parse_handle_accepts_uuid_and_rejects_garbage() {
        let id = Uuid::new_v4();
        assert_eq!(parse_handle(&id.to_string()).expect("valid uuid"), id);

        let err = parse_handle("not-a-uuid").unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        assert!(err.message.contains("not a valid source handle"));

        assert_eq!(parse_handle("").unwrap_err().kind, HexForgeErrorKind::InvalidInput);
    }

    #[test]
    fn scheduler_registry_exposes_merge_operation() {
        // Переехало в scheduler.rs вместе с build_snapshot; здесь остаётся
        // смоук-проверка, что реестр содержит merge-операцию планировщика.
        let registry = hexforge_ops::build_registry();
        assert!(registry.get_merge("streaming.concat").is_some());
    }

    /// Собирает граф root(sourceHandle) → base64-encode поверх реального
    /// реестра операций — без Tauri State, через чистый AppState.
    fn setup_chain(state: &AppState, literal: &[u8]) -> (NodeId, NodeId) {
        let source_handle =
            state.sources.write().insert(SourceEntry::InMemory(literal.to_vec()));

        let root_id = NodeId::new_v4();
        let encode_id = NodeId::new_v4();

        state.graph.write().insert_node(OperationNode {
            id: root_id,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
            inputs: vec![],
        });
        state.graph.write().insert_node(OperationNode {
            id: encode_id,
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![root_id],
        });
        (root_id, encode_id)
    }

    fn no_progress(_event: &hexforge_engine::scheduler::ProgressEvent) {}

    fn fresh_token() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn run_node_executes_and_records_history_for_whole_chain() {
        use base64::Engine as _;

        let state = AppState::new(hexforge_ops::build_registry());
        let (root_id, encode_id) = setup_chain(&state, b"Hello");

        let rot13_hello = rot13(b"Hello");
        let expected_b64 = general_purpose::STANDARD.encode(&rot13_hello);

        let output =
            scheduler::execute_chain(&state, &encode_id, &fresh_token(), &no_progress)
                .expect("chain must execute");
        assert_eq!(output.as_slice(), expected_b64.into_bytes().as_slice());

        {
            let history = state.history.read();
            assert_eq!(history.order.len(), 2, "one snapshot per executed node");
            let snaps = history.ordered_snapshots();
            assert_eq!(snaps[0].node_id, root_id);
            assert_eq!(snaps[1].node_id, encode_id);
            // Линейная MVP-цепочка: второй снапшот ссылается на первый родителем.
            assert_eq!(snaps[0].parent, None);
            assert_eq!(snaps[1].parent, Some(snaps[0].id));
            // Content-hash'и фиксируют фактические байты входа/выхода узла.
            assert_eq!(snaps[0].input_content_hash, blake3::hash(b"Hello"));
            assert_eq!(snaps[0].output_content_hash, Some(blake3::hash(&rot13_hello)));
            // Вход следующего узла — выход предыдущего (воспроизводимость FR-4.2).
            assert_eq!(snaps[1].input_content_hash, blake3::hash(&rot13_hello));
            assert_eq!(history.current, Some(snaps[1].id));
        }

        // list_snapshots отражает тот же журнал в том же порядке,
        // со строковыми UUID и hex-хэшами (контракт 05-IPC).
        let dtos = list_snapshots_inner(&state);
        assert_eq!(dtos.len(), 2);
        assert_eq!(dtos[0].node_id, root_id.to_string());
        assert_eq!(dtos[1].node_id, encode_id.to_string());
        assert_eq!(
            dtos[0].input_content_hash,
            blake3::hash(b"Hello").to_hex().to_string()
        );
        assert_eq!(dtos[1].parent.as_deref(), Some(dtos[0].id.as_str()));
    }

    fn rot13(data: &[u8]) -> Vec<u8> {
        data.iter()
            .copied()
            .map(|b| match b {
                b'a'..=b'z' => b'a' + (b - b'a' + 13) % 26,
                b'A'..=b'Z' => b'A' + (b - b'A' + 13) % 26,
                other => other,
            })
            .collect()
    }

    #[test]
    fn run_node_rejects_version_mismatch() {
        let state = AppState::new(hexforge_ops::build_registry());
        let node_id = NodeId::new_v4();
        let source_handle = state.sources.write().insert(SourceEntry::InMemory(b"x".to_vec()));
        state.graph.write().insert_node(OperationNode {
            id: node_id,
            operation_id: "text.rot13".into(),
            operation_version: "9.9.9".into(),
            params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
            inputs: vec![],
        });

        let err = scheduler::execute_chain(&state, &node_id, &fresh_token(), &no_progress).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::Internal);
        assert!(err.message.contains("version mismatch"));
        assert_eq!(err.node_id.as_deref(), Some(node_id.to_string().as_str()));
        // Упавшее выполнение не оставляет снапшотов в истории.
        assert!(state.history.read().order.is_empty());
    }

    #[test]
    fn run_node_rejects_unknown_operation() {
        let state = AppState::new(hexforge_ops::build_registry());
        let node_id = NodeId::new_v4();
        let source_handle = state.sources.write().insert(SourceEntry::InMemory(b"x".to_vec()));
        state.graph.write().insert_node(OperationNode {
            id: node_id,
            operation_id: "encoding.nonexistent".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
            inputs: vec![],
        });

        let err = scheduler::execute_chain(&state, &node_id, &fresh_token(), &no_progress).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::Internal);
        assert!(err.message.contains("unknown operation"));
    }

    // ---------- IPC-parity golden-тесты (05-IPC-CONTRACT.md) ----------
    // Каждый DTO сериализуется и сверяется с эталоном, зеркалящим
    // src/lib/ipc-contract.ts. Переименование поля, смена регистра или
    // лишнее поле роняют тест до того, как дрейф контракта увидит
    // фронтенд в рантайме. Сравнение через serde_json::Value —
    // порядок ключей в JSON не является частью контракта.

    #[test]
    fn error_wire_format_matches_ts_contract() {
        use hexforge_engine::error::HexForgeErrorKind;

        let err = HexForgeError::invalid_parameter("utf8", "too large");
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            serde_json::json!({
                "kind": "InvalidParameter",
                "message": "too large",
                "field": "utf8",
            })
        );

        // Опциональные поля не сериализуются вовсе, а не как null.
        assert_eq!(
            serde_json::to_value(HexForgeError::invalid_input("boom")).unwrap(),
            serde_json::json!({ "kind": "InvalidInput", "message": "boom" })
        );

        // Полный набор kind'ов обязан совпадать с TS-объединением
        // HexForgeErrorKind (регистр PascalCase — часть контракта).
        let kinds: Vec<String> = [
            HexForgeErrorKind::InvalidParameter,
            HexForgeErrorKind::InvalidInput,
            HexForgeErrorKind::MemoryBudgetExceeded,
            HexForgeErrorKind::CycleDetected,
            HexForgeErrorKind::DanglingInput,
            HexForgeErrorKind::PluginSignatureInvalid,
            HexForgeErrorKind::PluginCapabilityDenied,
            HexForgeErrorKind::Cancelled,
            HexForgeErrorKind::Internal,
        ]
        .iter()
        .map(|k| {
            serde_json::to_value(k)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(
            kinds,
            vec![
                "InvalidParameter",
                "InvalidInput",
                "MemoryBudgetExceeded",
                "CycleDetected",
                "DanglingInput",
                "PluginSignatureInvalid",
                "PluginCapabilityDenied",
                "Cancelled",
                "Internal",
            ]
        );

        // Wire format нового варианта: PascalCase-слово без изменений.
        assert_eq!(
            serde_json::to_value(HexForgeErrorKind::Cancelled).unwrap(),
            serde_json::json!("Cancelled")
        );
    }

    #[test]
    fn operation_descriptor_wire_format_matches_ts_contract() {
        let dto = OperationDescriptor {
            id: "encoding.base64.decode".into(),
            version: "1.0.0".into(),
            display_name: "Base64 Decode".into(),
            category: "Encoding".into(),
            params_schema: serde_json::json!({ "type": "object" }),
            capabilities: hexforge_core::TransformCapabilities {
                deterministic: true,
                streamable: false,
                memory_cost: hexforge_core::MemoryCost::FullBuffer,
            },
        };
        // Регрессия: memory_cost когда-то уходил как "memory_cost", тогда как
        // контракт требует "memoryCost" — фронтенд получал undefined.
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "id": "encoding.base64.decode",
                "version": "1.0.0",
                "displayName": "Base64 Decode",
                "category": "Encoding",
                "paramsSchema": { "type": "object" },
                "capabilities": {
                    "deterministic": true,
                    "streamable": false,
                    "memoryCost": "full_buffer",
                },
            })
        );
    }

    #[test]
    fn source_command_responses_match_ts_contract() {
        let open = OpenFileResponse {
            handle: "h1".into(),
            size_bytes: 7,
            detected_mime: Some("image/png".into()),
        };
        assert_eq!(
            serde_json::to_value(&open).unwrap(),
            serde_json::json!({ "handle": "h1", "sizeBytes": 7, "detectedMime": "image/png" })
        );

        let literal = CreateLiteralSourceResponse {
            handle: "h2".into(),
            size_bytes: 3,
        };
        assert_eq!(
            serde_json::to_value(&literal).unwrap(),
            serde_json::json!({ "handle": "h2", "sizeBytes": 3 })
        );

        let preview = PreviewBytesResponse {
            base64_chunk: "AAA=".into(),
            actual_length: 3,
        };
        assert_eq!(
            serde_json::to_value(&preview).unwrap(),
            serde_json::json!({ "base64Chunk": "AAA=", "actualLength": 3 })
        );
    }

    #[test]
    fn run_node_response_matches_ts_contract() {
        let resp = RunNodeResponse {
            output_handle: "h3".into(),
            output_size_bytes: 9,
            duration_ms: 12,
        };
        assert_eq!(
            serde_json::to_value(&resp).unwrap(),
            serde_json::json!({ "outputHandle": "h3", "outputSizeBytes": 9, "durationMs": 12 })
        );
    }

    #[test]
    fn snapshot_dto_matches_ts_contract() {
        let dto = SnapshotDto {
            id: "00000000-0000-4000-8000-000000000001".into(),
            parent: None,
            node_id: "00000000-0000-4000-8000-000000000002".into(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            input_content_hash: "aa".repeat(32),
            output_content_hash: Some("bb".repeat(32)),
        };
        // parent: null соответствует TS `SnapshotId | null`.
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "id": "00000000-0000-4000-8000-000000000001",
                "parent": null,
                "nodeId": "00000000-0000-4000-8000-000000000002",
                "operationId": "text.rot13",
                "operationVersion": "1.0.0",
                "params": {},
                "inputContentHash": "aa".repeat(32),
                "outputContentHash": "bb".repeat(32),
            })
        );
    }

    #[test]
    fn plugin_manifest_dto_matches_ts_contract() {
        let dto = PluginManifestDto {
            id: "plugin.example".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            author: "HexForge".into(),
            signature_valid: true,
            requested_capabilities: vec!["filesystem_read".into()],
            granted_capabilities: vec![],
        };
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "id": "plugin.example",
                "name": "Example",
                "version": "1.0.0",
                "author": "HexForge",
                "signatureValid": true,
                "requestedCapabilities": ["filesystem_read"],
                "grantedCapabilities": [],
            })
        );
    }

    #[test]
    fn sort_for_palette_is_deterministic() {
        let mk = |id: &str, cat: &str, name: &str| OperationDescriptor {
            id: id.into(),
            version: "1.0.0".into(),
            display_name: name.into(),
            category: cat.into(),
            params_schema: serde_json::json!({}),
            capabilities: hexforge_core::TransformCapabilities {
                deterministic: true,
                streamable: false,
                memory_cost: hexforge_core::MemoryCost::FullBuffer,
            },
        };

        // Перестановки одного набора дают идентичный порядок.
        let mut v1 = vec![
            mk("c", "Encoding", "To Hex"),
            mk("a", "Encoding", "Base64 Decode"),
            mk("b", "Hashing", "MD5"),
        ];
        sort_for_palette(&mut v1);

        let mut v2 = vec![
            mk("b", "Hashing", "MD5"),
            mk("a", "Encoding", "Base64 Decode"),
            mk("c", "Encoding", "To Hex"),
        ];
        sort_for_palette(&mut v2);

        let ids: Vec<String> = v1.iter().map(|d| d.id.clone()).collect();
        let ids2: Vec<String> = v2.iter().map(|d| d.id.clone()).collect();
        assert_eq!(ids, ids2);
        // Encoding < Hashing; внутри категории Base64 Decode < To Hex.
        assert_eq!(ids, vec!["a", "c", "b"]);

        // Тай-брейк по id при одинаковых category+name.
        let mut v3 = vec![mk("zz", "T", "Same"), mk("aa", "T", "Same")];
        sort_for_palette(&mut v3);
        assert_eq!(v3[0].id, "aa");
    }

    #[test]
    fn patch_source_dtos_match_ts_contract() {
        let req = PatchSourceRequest {
            handle: "h1".into(),
            offset: 4096,
            bytes_base64: "AQI=".into(), // [1, 2]
        };
        assert_eq!(
            serde_json::to_value(&req).unwrap(),
            serde_json::json!({
                "handle": "h1",
                "offset": 4096,
                "bytesBase64": "AQI=",
            })
        );

        let resp = PatchSourceResponse { new_size_bytes: 8192 };
        assert_eq!(
            serde_json::to_value(&resp).unwrap(),
            serde_json::json!({ "newSizeBytes": 8192 })
        );
    }

    #[test]
    fn graph_invalidated_event_matches_ts_contract() {
        use hexforge_engine::scheduler::GraphInvalidatedEvent;
        let ev = GraphInvalidatedEvent {
            stale_node_ids: vec![
                "00000000-0000-4000-8000-00000000000b".into(),
                "00000000-0000-4000-8000-00000000000c".into(),
            ],
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::json!({
                "staleNodeIds": [
                    "00000000-0000-4000-8000-00000000000b",
                    "00000000-0000-4000-8000-00000000000c"
                ],
            })
        );
    }

    #[test]
    fn progress_event_matches_ts_contract() {
        use hexforge_engine::scheduler::ProgressEvent;
        let ev = ProgressEvent {
            node_id: "00000000-0000-4000-8000-00000000000a".into(),
            bytes_processed: 42,
            bytes_total: None,
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::json!({
                "nodeId": "00000000-0000-4000-8000-00000000000a",
                "bytesProcessed": 42,
                "bytesTotal": null,
            })
        );
    }

    #[test]
    fn graph_dto_accepts_camel_case_payload() {
        let node_id = Uuid::new_v4();
        let mut nodes = serde_json::Map::new();
        nodes.insert(
            node_id.to_string(),
            serde_json::json!({
                "id": node_id.to_string(),
                "operationId": "text.rot13",
                "operationVersion": "1.0.0",
                "params": {},
                "inputs": [],
            }),
        );
        let payload = serde_json::Value::Object(
            std::iter::once(("nodes".to_string(), serde_json::Value::Object(nodes))).collect(),
        );

        let dto: GraphDto =
            serde_json::from_value(payload).expect("camelCase graph payload must deserialize");
        assert!(dto.nodes.contains_key(&node_id.to_string()));

        // И конвертация во внутренний Graph работает без сюрпризов.
        let graph = Graph::try_from(dto).expect("valid dto must convert");
        assert!(graph.nodes.contains_key(&node_id));
    }

    fn chain_dto(root_id: Uuid, encode_id: Uuid, source_handle: &Uuid) -> GraphDto {
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(
            root_id.to_string(),
            OperationNodeDto {
                id: root_id.to_string(),
                operation_id: "text.rot13".into(),
                operation_version: "1.0.0".into(),
                params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
                inputs: vec![],
            },
        );
        nodes.insert(
            encode_id.to_string(),
            OperationNodeDto {
                id: encode_id.to_string(),
                operation_id: "encoding.base64.encode".into(),
                operation_version: "1.0.0".into(),
                params: serde_json::json!({}),
                inputs: vec![root_id.to_string()],
            },
        );
        GraphDto { nodes }
    }

    #[test]
    fn export_import_roundtrip() {
        let state = AppState::new(hexforge_ops::build_registry());
        let source = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(b"abc".to_vec()));
        let root_id = Uuid::new_v4();
        let encode_id = Uuid::new_v4();
        let dto = chain_dto(root_id, encode_id, &source);

        let path = std::env::temp_dir().join(format!("hexforge-recipe-{}.json", Uuid::new_v4()));
        let _guard = DropGuard(path.clone());

        export_recipe_inner(&state, ExportRecipeRequest {
            graph: dto.clone(),
            target_path: path.to_string_lossy().into_owned(),
        })
        .expect("reproducible recipe must export");

        // Файл — валидный JSON структуры GraphDto (camelCase).
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"operationId\""));

        let resp = import_recipe_inner(
            &state,
            ImportRecipeRequest {
                source_path: path.to_string_lossy().into_owned(),
            },
        )
        .expect("own recipe must import");
        assert_eq!(
            resp.missing_operations,
            Vec::<String>::new(),
            "all operations are built-in"
        );
        assert_eq!(resp.graph.nodes.len(), 2);
        assert!(resp.graph.nodes.contains_key(&root_id.to_string()));
        assert!(resp.graph.nodes.contains_key(&encode_id.to_string()));
    }

    #[test]
    fn import_reports_missing_operations() {
        let state = AppState::new(hexforge_ops::build_registry());
        let root_id = Uuid::new_v4();

        let mut nodes = std::collections::HashMap::new();
        nodes.insert(
            root_id.to_string(),
            OperationNodeDto {
                id: root_id.to_string(),
                operation_id: "encoding.nonexistent".into(),
                operation_version: "1.0.0".into(),
                params: serde_json::json!({}),
                inputs: vec![],
            },
        );

        let path = std::env::temp_dir().join(format!("hexforge-recipe-miss-{}.json", Uuid::new_v4()));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&GraphDto { nodes }).unwrap(),
        )
        .unwrap();

        let resp = import_recipe_inner(
            &state,
            ImportRecipeRequest {
                source_path: path.to_string_lossy().into_owned(),
            },
        )
        .expect("import succeeds even with missing ops");
        assert_eq!(resp.missing_operations, vec!["encoding.nonexistent"]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_rejects_unknown_operation_upfront() {
        let state = AppState::new(hexforge_ops::build_registry());
        let node_id = Uuid::new_v4();
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(
            node_id.to_string(),
            OperationNodeDto {
                id: node_id.to_string(),
                operation_id: "encoding.nonexistent".into(),
                operation_version: "1.0.0".into(),
                params: serde_json::json!({}),
                inputs: vec![],
            },
        );

        let err = export_recipe_inner(
            &state,
            ExportRecipeRequest {
                graph: GraphDto { nodes },
                target_path: std::env::temp_dir()
                    .join(format!("hexforge-nope-{}.json", Uuid::new_v4()))
                    .to_string_lossy()
                    .into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        assert!(err.message.contains("missing from registry"));
    }

    #[test]
    fn import_rejects_invalid_json_and_cycles() {
        let state = AppState::new(hexforge_ops::build_registry());

        let bad_json =
            std::env::temp_dir().join(format!("hexforge-bad-{}.json", Uuid::new_v4()));
        std::fs::write(&bad_json, "{ not json").unwrap();
        let err = import_recipe_inner(
            &state,
            ImportRecipeRequest {
                source_path: bad_json.to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        let _ = std::fs::remove_file(&bad_json);

        // Цикл валиден как JSON, но отвергается проверкой DAG.
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let cycle_node = |id: Uuid, input: Uuid| OperationNodeDto {
            id: id.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![input.to_string()],
        };
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(a.to_string(), cycle_node(a, b));
        nodes.insert(b.to_string(), cycle_node(b, a));

        let cycle_file =
            std::env::temp_dir().join(format!("hexforge-cycle-{}.json", Uuid::new_v4()));
        std::fs::write(
            &cycle_file,
            serde_json::to_string_pretty(&GraphDto { nodes }).unwrap(),
        )
        .unwrap();
        let err = import_recipe_inner(
            &state,
            ImportRecipeRequest {
                source_path: cycle_file.to_string_lossy().into_owned(),
            },
        )
        .unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::CycleDetected);
        let _ = std::fs::remove_file(&cycle_file);
    }

    /// Убирает временный файл даже при панике ассертов.
    struct DropGuard(std::path::PathBuf);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
