//! Реализация Tauri commands. Каждая структура запроса/ответа здесь —
//! зеркало соответствующего типа в `src/lib/ipc-contract.ts`
//! (см. `05-IPC-CONTRACT.md`). `#[serde(rename_all = "camelCase")]`
//! обеспечивает совпадение имён полей с TS без ручного маппинга.

use base64::{engine::general_purpose, Engine as _};
use hexforge_core::graph::{Graph, NodeId};
use hexforge_engine::error::{HexForgeError, HexForgeResult};
use hexforge_engine::graph_dto::{GraphDto, MissingPlugin, PluginDependency};
use hexforge_engine::scheduler;
use hexforge_engine::state::{AppState, SourceEntry, WriteRegionError};
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
    /// Operation origin: `"builtin"` or `"plugin"`. Additive — older
    /// consumers ignore it; the palette uses it for the plugin badge.
    pub origin: String,
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
    list_operations_inner(state.inner())
}

fn list_operations_inner(state: &AppState) -> Vec<OperationDescriptor> {
    let mut descriptors: Vec<OperationDescriptor> = state
        .registry
        .read()
        .iter()
        .map(|t| OperationDescriptor {
            id: t.id().to_string(),
            version: t.version().to_string(),
            display_name: t.display_name().to_string(),
            category: t.category().to_string(),
            params_schema: t.params_schema(),
            capabilities: t.capabilities(),
            origin: t.origin().to_string(),
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
    validate_fs_path(&req.path, "path")?;
    let file = std::fs::File::open(&req.path)
        .map_err(|e| HexForgeError::invalid_input(format!("cannot open '{}': {e}", req.path)))?;

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
    let entry = sources.get(&handle).ok_or_else(|| {
        HexForgeError::invalid_input(format!("unknown source handle: {}", req.handle))
    })?;

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
pub fn export_output(
    req: ExportOutputRequest,
    state: State<Arc<AppState>>,
) -> HexForgeResult<ExportOutputResponse> {
    export_output_inner(state.inner(), req)
}

fn export_output_inner(
    state: &AppState,
    req: ExportOutputRequest,
) -> HexForgeResult<ExportOutputResponse> {
    use std::io::Write;
    let handle = parse_handle(&req.handle)?;
    let sources = state.sources.read();
    let entry = sources.get(&handle).ok_or_else(|| {
        HexForgeError::invalid_input(format!("unknown source handle: {}", req.handle))
    })?;

    const CHUNK_SIZE: usize = 64 * 1024 * 1024; // 64 MB
    let bytes = entry.as_bytes();
    let total = bytes.len();

    let mut file = std::fs::File::create(&req.target_path).map_err(|e| {
        HexForgeError::internal(format!("cannot create '{}': {e}", req.target_path))
    })?;

    let mut written: usize = 0;
    while written < total {
        let end = (written + CHUNK_SIZE).min(total);
        file.write_all(&bytes[written..end])
            .map_err(|e| HexForgeError::internal(format!("write @{written}: {e}")))?;
        written = end;
    }

    file.flush()
        .map_err(|e| HexForgeError::internal(format!("flush: {e}")))?;

    Ok(ExportOutputResponse {
        bytes_written: written,
    })
}

#[tauri::command]
pub fn release_source(req: ReleaseSourceRequest, state: State<Arc<AppState>>) -> bool {
    match parse_handle(&req.handle) {
        Ok(handle) => state.sources.write().release(&handle),
        Err(_) => false,
    }
}

fn parse_handle(raw: &str) -> HexForgeResult<Uuid> {
    Uuid::parse_str(raw)
        .map_err(|_| HexForgeError::invalid_input(format!("'{raw}' is not a valid source handle")))
}

fn validate_fs_path(path: &str, field: &str) -> HexForgeResult<()> {
    if path.is_empty() {
        return Err(HexForgeError::invalid_parameter(
            field,
            "path must not be empty",
        ));
    }
    if path.len() > 4096 {
        return Err(HexForgeError::invalid_parameter(
            field,
            "path exceeds maximum length (4096)",
        ));
    }
    if path.contains('\0') {
        return Err(HexForgeError::invalid_parameter(
            field,
            "path contains null byte",
        ));
    }
    Ok(())
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
pub async fn patch_source(
    req: PatchSourceRequest,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
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
    })?;

    // Консервативная инвалидация кэша (см. OutputCache::clear): патч меняет
    // байты за хэндлом — прежние content-hash ключи больше не соответствуют.
    state.cache.lock().clear();

    // FR-1.6: потребители патчнутого источника устарели — уведомляем UI.
    let stale = hexforge_engine::scheduler::compute_invalidated_for_source(
        &state.graph.read(),
        &req.handle,
    );
    if !stale.is_empty() {
        use tauri::Emitter;
        let _ = app.emit(
            "graph://invalidated",
            hexforge_engine::scheduler::GraphInvalidatedEvent {
                stale_node_ids: stale,
            },
        );
    }

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
            hexforge_engine::scheduler::GraphInvalidatedEvent {
                stale_node_ids: stale,
            },
        );
    }
    Ok(())
}

// ---------- Выполнение узла ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunNodeRequest {
    pub node_id: String,
    /// FR-1.6: `previewOnly=true` — пересчитывается только запрошенный узел и его
    /// входная цепочка; `false` — дополнительно прогреваются (кэшируются)
    /// downstream-узлы для мгновенного превью при переключении (контракт 05 §3).
    pub preview_only: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunNodeResponse {
    pub output_handle: String,
    pub output_size_bytes: u64,
    pub duration_ms: u64,
}

/// Исполнитель одного узла поверх планировщика `hexforge-engine`: рекурсивное
/// исполнение входной цепочки через `scheduler::execute_chain` с memoization
/// по `reproducibility_key`, chunked streaming для streamable-операций,
/// merge-ветками через `MergeTransform` и кооперативной отменой.
/// Каждый узел цепочки пишет Snapshot истории; при `preview_only=false`
/// дополнительно прогреваются downstream-узлы для мгновенного превью.
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
    let app_for_progress = app.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        // Прогресс уходит в WebView; ошибка доставки сознательно игнорируется.
        let on_progress = |event: &hexforge_engine::scheduler::ProgressEvent| {
            let _ = app_for_progress.emit("op://progress", event);
        };
        scheduler::execute_chain(&task_state, &node_id, &task_token, &on_progress)
    })
    .await
    .map_err(|e| HexForgeError::internal(format!("node execution worker failed: {e}")))??;

    // FR-1.6: preview_only=false — прогреваем downstream кэш конкурентно
    // (мгновенное превью при переключении узлов). Ранее warming был
    // последовательным (`for ... spawn_blocking().await`), что для fork-графов
    // с N ветвями давало N× latency. Теперь все downstream узлы прогреваются
    // параллельно через blocking-пул, ошибки игнорируются, отмена проверяется
    // до спауна и между join.
    if !req.preview_only {
        let downstream: Vec<Uuid> = {
            let g = exec_state.graph.read().clone();
            g.downstream_of(node_id)
                .into_iter()
                .filter(|id| *id != node_id)
                .collect()
        };
        if !downstream.is_empty() {
            let mut handles = Vec::with_capacity(downstream.len());
            for down_id in downstream {
                if token.load(Ordering::Relaxed) {
                    break;
                }
                let t_state = Arc::clone(&exec_state);
                let t_token = Arc::clone(&token);
                let app_clone = app.clone();
                handles.push(tauri::async_runtime::spawn_blocking(move || {
                    let on_progress = |event: &hexforge_engine::scheduler::ProgressEvent| {
                        let _ = app_clone.emit("op://progress", event);
                    };
                    let _ = scheduler::execute_chain(&t_state, &down_id, &t_token, &on_progress);
                }));
            }
            for h in handles {
                let _ = h.await;
                if token.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

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

// ---------- Streaming export (FR-5.4) ----------
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutputRequest {
    pub handle: String,
    pub target_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutputResponse {
    pub bytes_written: usize,
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
    /// Declared plugin dependencies that are not installed or version-match —
    /// warning metadata, never hidden; execution still enforces strictly.
    pub missing_plugins: Vec<MissingPlugin>,
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
    validate_fs_path(&req.target_path, "targetPath")?;
    let graph: Graph = req.graph.clone().try_into()?;
    graph.topo_order().map_err(HexForgeError::from)?;

    let mut missing: Vec<String> = Vec::new();
    for node in graph.nodes.values() {
        let reproducible = state
            .registry
            .read()
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

    // Plugin dependencies are server-computed metadata (never trusted from
    // the client dto): every `plugin:<id>` node pins its exact version.
    let mut dto = req.graph.clone();
    dto.required_plugins = collect_required_plugins(&graph);
    let json = serde_json::to_string_pretty(&dto)
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

fn import_recipe_inner(
    state: &AppState,
    req: ImportRecipeRequest,
) -> HexForgeResult<ImportRecipeResponse> {
    validate_fs_path(&req.source_path, "sourcePath")?;
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
            .read()
            .get(&node.operation_id)
            .map(|t| t.version() == node.operation_version)
            .unwrap_or(false);
        if !known {
            missing.insert(node.operation_id.clone());
        }
    }

    let missing_plugins = check_required_plugins(state, &dto.required_plugins);
    Ok(ImportRecipeResponse {
        graph: dto,
        missing_operations: missing.into_iter().collect(),
        missing_plugins,
    })
}

/// Collects canonical plugin dependencies from graph nodes: every
/// `plugin:<id>` node contributes its pinned (id, version). Deduped and
/// sorted — the same plugin on N nodes is declared once.
fn collect_required_plugins(graph: &hexforge_core::graph::Graph) -> Vec<PluginDependency> {
    let mut deps = std::collections::BTreeSet::new();
    for node in graph.nodes.values() {
        if node.operation_id.starts_with("plugin:") {
            deps.insert(PluginDependency {
                id: node.operation_id.clone(),
                version: node.operation_version.clone(),
            });
        }
    }
    deps.into_iter().collect()
}

/// Checks declared plugin dependencies against the live registry: missing
/// installs and version mismatches are reported (never hidden); hard
/// enforcement still happens at `run_node` via the strict version gate.
fn check_required_plugins(state: &AppState, deps: &[PluginDependency]) -> Vec<MissingPlugin> {
    let registry = state.registry.read();
    let mut out = Vec::new();
    for dep in deps {
        match registry.get(&dep.id) {
            None => out.push(MissingPlugin {
                id: dep.id.clone(),
                version: dep.version.clone(),
                reason: format!(
                    "plugin '{}' is not installed: install version {} and re-import",
                    dep.id, dep.version
                ),
            }),
            Some(t) if t.version() != dep.version => out.push(MissingPlugin {
                id: dep.id.clone(),
                version: dep.version.clone(),
                reason: format!(
                    "plugin '{}' version mismatch: recipe requires {}, installed {}",
                    dep.id,
                    dep.version,
                    t.version()
                ),
            }),
            _ => {}
        }
    }
    out
}

#[derive(Debug, Serialize, Deserialize)]
struct CyberChefOp {
    op: String,
    args: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCyberChefRecipeRequest {
    pub source_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCyberChefRecipeResponse {
    pub graph: GraphDto,
    pub unmapped_operations: Vec<UnmappedOp>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCyberChefRecipeResponse {
    pub content: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmappedOp {
    pub cyber_chef_id: String,
    pub reason: String,
}

/// Экспорт графа в формат CyberChef recipe (JSON-массив [{ "op": "...", "args": [...] }]).
/// CyberChef recipe — линейная последовательность операций; граф, который не является
/// single linear chain (fork/merge/multi-input), отклоняется с диагностикой, а не молча
/// усекается — это предотвращает silent data loss. Не-маппируемые операции попадают в
/// warnings (файл всё ещё создаётся, но пользователь видит, что часть операций не
/// экспортировалась — best-effort по FR-7.2).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCyberChefRecipeRequest {
    pub graph: GraphDto,
    pub target_path: String,
}

fn cyberchef_export_op_name(hexforge_id: &str, params: &serde_json::Value) -> Option<CyberChefOp> {
    match hexforge_id {
        "encoding.base64.encode" => Some(CyberChefOp {
            op: "To Base64".into(),
            args: vec![],
        }),
        "encoding.base64.decode" => Some(CyberChefOp {
            op: "From Base64".into(),
            args: vec![],
        }),
        "encoding.hex.encode" => Some(CyberChefOp {
            op: "To Hex".into(),
            args: vec![],
        }),
        "encoding.hex.decode" => Some(CyberChefOp {
            op: "From Hex".into(),
            args: vec![],
        }),
        "encoding.base32.encode" => Some(CyberChefOp {
            op: "To Base32".into(),
            args: vec![],
        }),
        "encoding.base32.decode" => Some(CyberChefOp {
            op: "From Base32".into(),
            args: vec![],
        }),
        "text.rot13" => Some(CyberChefOp {
            op: "ROT13".into(),
            args: vec![],
        }),
        "text.reverse" => Some(CyberChefOp {
            op: "Reverse".into(),
            args: vec![],
        }),
        "network.url_encode" => Some(CyberChefOp {
            op: "URL Encode".into(),
            args: vec![],
        }),
        "network.url_decode" => Some(CyberChefOp {
            op: "URL Decode".into(),
            args: vec![],
        }),
        "compression.gzip.compress" => Some(CyberChefOp {
            op: "Gzip Compress".into(),
            args: vec![],
        }),
        "compression.gzip.decompress" => Some(CyberChefOp {
            op: "Gzip Decompress".into(),
            args: vec![],
        }),
        "compression.zlib.compress" => Some(CyberChefOp {
            op: "Zlib Deflate".into(),
            args: vec![],
        }),
        "compression.zlib.decompress" => Some(CyberChefOp {
            op: "Zlib Inflate".into(),
            args: vec![],
        }),
        "compression.bzip2.compress" => Some(CyberChefOp {
            op: "Bzip2 Compress".into(),
            args: vec![],
        }),
        "compression.bzip2.decompress" => Some(CyberChefOp {
            op: "Bzip2 Decompress".into(),
            args: vec![],
        }),
        "compression.lzma.compress" => Some(CyberChefOp {
            op: "LZMA Compress".into(),
            args: vec![],
        }),
        "compression.lzma.decompress" => Some(CyberChefOp {
            op: "LZMA Decompress".into(),
            args: vec![],
        }),
        "crypto.xor" => {
            let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("key");
            Some(CyberChefOp {
                op: "XOR".into(),
                args: vec![serde_json::json!({"key": key})],
            })
        }
        "hashing.md5" => Some(CyberChefOp {
            op: "MD5".into(),
            args: vec![],
        }),
        "hashing.sha1" => Some(CyberChefOp {
            op: "SHA1".into(),
            args: vec![],
        }),
        "hashing.sha256" => Some(CyberChefOp {
            op: "SHA256".into(),
            args: vec![],
        }),
        "hashing.sha512" => Some(CyberChefOp {
            op: "SHA512".into(),
            args: vec![],
        }),
        "hashing.sha3_256" => Some(CyberChefOp {
            op: "SHA3".into(),
            args: vec![serde_json::json!(256)],
        }),
        "hashing.blake2b" => Some(CyberChefOp {
            op: "BLAKE2b".into(),
            args: vec![],
        }),
        "hashing.blake2s" => Some(CyberChefOp {
            op: "BLAKE2s".into(),
            args: vec![],
        }),
        "hashing.blake3" => Some(CyberChefOp {
            op: "BLAKE3".into(),
            args: vec![],
        }),
        "hashing.crc32" => Some(CyberChefOp {
            op: "CRC32".into(),
            args: vec![],
        }),
        "hashing.ssdeep" | "hashing.ssdeep_fuzzy" => Some(CyberChefOp {
            op: "SSDEEP".into(),
            args: vec![],
        }),
        "binary.entropy" => Some(CyberChefOp {
            op: "Entropy".into(),
            args: vec![],
        }),
        "binary.strings_extract" => Some(CyberChefOp {
            op: "Strings".into(),
            args: vec![],
        }),
        "binary.magic" => Some(CyberChefOp {
            op: "Detect File Type".into(),
            args: vec![],
        }),
        _ => None,
    }
}

fn validate_cyberchef_exportable(graph: &Graph) -> Result<(), String> {
    let sources: Vec<_> = graph
        .nodes
        .values()
        .filter(|n| n.inputs.is_empty())
        .collect();
    if sources.len() != 1 {
        return Err(format!(
            "expected exactly one source node (no inputs) for CyberChef export; found {}",
            sources.len()
        ));
    }
    let mut children: std::collections::HashMap<NodeId, Vec<NodeId>> =
        std::collections::HashMap::new();

    for node in graph.nodes.values() {
        if !node.inputs.is_empty() && node.inputs.len() != 1 {
            return Err(format!(
                "node '{}' has {} inputs (CyberChef recipe is linear; fork/merge not supported)",
                node.id,
                node.inputs.len()
            ));
        }
        for input_id in &node.inputs {
            children.entry(*input_id).or_default().push(node.id);
        }
    }

    for (&parent, child_ids) in &children {
        if child_ids.len() > 1 {
            return Err(format!(
                "fork at node '{}': {} downstream nodes (CyberChef recipe is linear; fork not supported)",
                parent, child_ids.len()
            ));
        }
    }
    let reachable = graph.downstream_of(sources[0].id);
    if reachable.len() != graph.nodes.len() {
        return Err(format!(
            "graph has {} unreachable nodes from source (not a single connected linear chain)",
            graph.nodes.len() - reachable.len()
        ));
    }
    Ok(())
}

/// Экспорт рецепта в CyberChef-совместимый JSON (миграционный мост, FR-7.2).
/// Валидирует, что граф является single linear chain (source → … → sink), иначе
/// возвращает ошибку с причиной. Неподдерживаемые операции попадают в warnings
/// (файл сохраняется, но пользователь видит диагностику).
#[tauri::command]
pub fn cyberchef_export_inner(
    req: ExportCyberChefRecipeRequest,
    state: &AppState,
) -> HexForgeResult<ExportCyberChefRecipeResponse> {
    validate_fs_path(&req.target_path, "targetPath")?;
    let graph: Graph = req.graph.clone().try_into()?;
    graph.topo_order().map_err(HexForgeError::from)?;
    validate_cyberchef_exportable(&graph)
        .map_err(|e| HexForgeError::invalid_input(format!("cannot export to CyberChef: {e}")))?;
    let _registry = state.registry.read();
    let mut ops = Vec::new();
    let mut warnings = Vec::new();
    for node_id in graph.topo_order().map_err(HexForgeError::from)? {
        let node = graph.nodes.get(&node_id).unwrap();
        match cyberchef_export_op_name(&node.operation_id, &node.params) {
            Some(cyber_op) => ops.push(cyber_op),
            None => warnings.push(format!(
                "cannot map operation '{}' to any CyberChef op; omitted",
                node.operation_id
            )),
        }
    }
    if ops.is_empty() {
        return Err(HexForgeError::invalid_input(
            "no CyberChef-mappable operations in graph",
        ));
    }
    let content = serde_json::to_string_pretty(&ops)
        .map_err(|e| HexForgeError::internal(format!("recipe serialization failed: {e}")))?;
    std::fs::write(&req.target_path, content.as_bytes()).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot write '{}': {e}", req.target_path))
    })?;
    Ok(ExportCyberChefRecipeResponse { content, warnings })
}

#[tauri::command]
pub fn export_cyberchef_recipe(
    req: ExportCyberChefRecipeRequest,
    state: State<'_, Arc<AppState>>,
) -> HexForgeResult<ExportCyberChefRecipeResponse> {
    cyberchef_export_inner(req, &state)
}

fn map_cyberchef_op(op: &str, _args: &[serde_json::Value]) -> Option<(String, serde_json::Value)> {
    match op {
        "To Base64" => Some(("encoding.base64.encode".into(), serde_json::json!({}))),
        "From Base64" => Some(("encoding.base64.decode".into(), serde_json::json!({}))),
        "To Hex" => Some(("encoding.hex.encode".into(), serde_json::json!({}))),
        "From Hex" => Some(("encoding.hex.decode".into(), serde_json::json!({}))),
        "To Base32" => Some(("encoding.base32.encode".into(), serde_json::json!({}))),
        "From Base32" => Some(("encoding.base32.decode".into(), serde_json::json!({}))),
        "ROT13" => Some(("text.rot13".into(), serde_json::json!({}))),
        "Reverse" => Some(("text.reverse".into(), serde_json::json!({}))),
        "URL Encode" => Some(("network.url_encode".into(), serde_json::json!({}))),
        "URL Decode" => Some(("network.url_decode".into(), serde_json::json!({}))),
        "Gzip Compress" => Some(("compression.gzip.compress".into(), serde_json::json!({}))),
        "Gzip Decompress" => Some(("compression.gzip.decompress".into(), serde_json::json!({}))),
        "Zlib Deflate" => Some(("compression.zlib.compress".into(), serde_json::json!({}))),
        "Zlib Inflate" => Some(("compression.zlib.decompress".into(), serde_json::json!({}))),
        "Bzip2 Compress" => Some(("compression.bzip2.compress".into(), serde_json::json!({}))),
        "Bzip2 Decompress" => Some(("compression.bzip2.decompress".into(), serde_json::json!({}))),
        "LZMA Compress" => Some(("compression.lzma.compress".into(), serde_json::json!({}))),
        "LZMA Decompress" => Some(("compression.lzma.decompress".into(), serde_json::json!({}))),
        "XOR" => {
            let key = _args.first().and_then(|v| v.as_str()).unwrap_or("key");
            Some(("crypto.xor".into(), serde_json::json!({"key": key})))
        }
        "MD5" => Some(("hashing.md5".into(), serde_json::json!({}))),
        "SHA1" => Some(("hashing.sha1".into(), serde_json::json!({}))),
        "SHA2" => {
            let bits = _args.first().and_then(|v| v.as_u64()).unwrap_or(256);
            match bits {
                512 => Some(("hashing.sha512".into(), serde_json::json!({}))),
                384 => Some(("hashing.sha512".into(), serde_json::json!({}))), // fallback: no sha384 op, use sha512
                224 => Some(("hashing.sha256".into(), serde_json::json!({}))),
                _ => Some(("hashing.sha256".into(), serde_json::json!({}))),
            }
        }
        "SHA256" => Some(("hashing.sha256".into(), serde_json::json!({}))),
        "SHA512" => Some(("hashing.sha512".into(), serde_json::json!({}))),
        "SHA3" => Some(("hashing.sha3_256".into(), serde_json::json!({}))),
        "BLAKE2b" => Some(("hashing.blake2b".into(), serde_json::json!({}))),
        "BLAKE2s" => Some(("hashing.blake2s".into(), serde_json::json!({}))),
        "BLAKE3" => Some(("hashing.blake3".into(), serde_json::json!({}))),
        "CRC32" => Some(("hashing.crc32".into(), serde_json::json!({}))),
        "SSDEEP" | "SSDeep" => Some(("hashing.ssdeep".into(), serde_json::json!({}))),
        "Entropy" => Some(("binary.entropy".into(), serde_json::json!({}))),
        "Strings" => Some(("binary.strings_extract".into(), serde_json::json!({}))),
        "Detect File Type" | "Magic" => Some(("binary.magic".into(), serde_json::json!({}))),
        _ => None,
    }
}

#[tauri::command]
pub fn import_cyberchef_recipe(
    req: ImportCyberChefRecipeRequest,
    _state: State<Arc<AppState>>,
) -> HexForgeResult<ImportCyberChefRecipeResponse> {
    validate_fs_path(&req.source_path, "sourcePath")?;
    let text = std::fs::read_to_string(&req.source_path).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot read '{}': {e}", req.source_path))
    })?;
    let ops: Vec<CyberChefOp> = serde_json::from_str(&text).map_err(|e| {
        HexForgeError::invalid_input(format!(
            "'{}' is not a valid CyberChef recipe: {e}",
            req.source_path
        ))
    })?;
    let mut nodes: std::collections::HashMap<String, hexforge_engine::graph_dto::OperationNodeDto> =
        std::collections::HashMap::new();
    let mut unmapped = Vec::new();
    let mut prev_id: Option<String> = None;
    for op in ops {
        if let Some((hex_id, params)) = map_cyberchef_op(&op.op, &op.args) {
            let id = Uuid::new_v4().to_string();
            let inputs = prev_id.clone().into_iter().collect();
            nodes.insert(
                id.clone(),
                hexforge_engine::graph_dto::OperationNodeDto {
                    id: id.clone(),
                    operation_id: hex_id,
                    operation_version: "1.0.0".into(),
                    params,
                    inputs,
                },
            );
            prev_id = Some(id);
        } else {
            unmapped.push(UnmappedOp {
                cyber_chef_id: op.op.clone(),
                reason: "no HexForge equivalent".into(),
            });
        }
    }
    // Validate resulting graph is DAG (linear chain always is, but check)
    let graph = GraphDto {
        nodes: nodes.clone(),
        required_plugins: Vec::new(),
    };
    let g: Graph = graph.clone().try_into()?;
    g.topo_order().map_err(HexForgeError::from)?;
    Ok(ImportCyberChefRecipeResponse {
        graph,
        unmapped_operations: unmapped,
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
    pub input_content_hashes: Option<Vec<String>>,
    pub input_snapshot_ids: Vec<String>,
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
            input_content_hashes: s
                .input_content_hashes
                .as_ref()
                .map(|v| v.iter().map(|h| h.to_hex().to_string()).collect()),
            input_snapshot_ids: s
                .input_snapshot_ids
                .iter()
                .map(|id| id.to_string())
                .collect(),
            output_content_hash: s.output_content_hash.map(|h| h.to_hex().to_string()),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSnapshotsRequest {
    pub a_snapshot_id: String,
    pub b_snapshot_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSnapshotsResponse {
    pub diff_text: String,
}

#[tauri::command]
pub async fn diff_snapshots(
    req: DiffSnapshotsRequest,
    state: State<'_, Arc<AppState>>,
) -> HexForgeResult<DiffSnapshotsResponse> {
    let a = parse_handle(&req.a_snapshot_id)?;
    let b = parse_handle(&req.b_snapshot_id)?;
    let exec_state = Arc::clone(state.inner());
    let diff =
        tauri::async_runtime::spawn_blocking(move || scheduler::diff_snapshots(&exec_state, a, b))
            .await
            .map_err(|e| HexForgeError::internal(format!("diff worker failed: {e}")))??;
    Ok(DiffSnapshotsResponse { diff_text: diff })
}

// ---------- Плагины (FR-6, NFR-9) — production MVP ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestDto {
    pub id: String,
    /// Human-readable name from the manifest (`PluginTransform::display_name`
    /// resolves WIT `get-display-name` when the binary is instantiated, but
    /// discovery never executes WASM — the list path reports the manifest
    /// value so the UI has one backend-owned source of truth).
    pub display_name: String,
    pub name: String,
    pub version: String,
    /// Transform category (`PluginTransform::category`: WIT `get-category`
    /// or the `"Plugin"` fallback; same no-execute caveat as display_name).
    pub category: String,
    pub author: String,
    pub signature_valid: bool,
    /// Discovery verdict (`verified` | `invalid` | `unavailable` |
    /// `incompatible`): the single backend-owned source for the PluginPanel
    /// status badge. `signature_valid` mirrors `status == "verified"`.
    pub status: String,
    pub requested_capabilities: Vec<String>,
    pub granted_capabilities: Vec<String>,
}

/// Single constructor for the plugin DTO so `list_plugins` and
/// `install_plugin` cannot drift (same fallback semantics by construction).
fn plugin_manifest_dto(
    manifest: hexforge_plugin_host::PluginManifest,
    signature_valid: bool,
    status: hexforge_plugin_host::store::PluginStatus,
) -> PluginManifestDto {
    PluginManifestDto {
        display_name: manifest.name.clone(),
        name: manifest.name.clone(),
        category: "Plugin".to_string(),
        id: manifest.id,
        version: manifest.version,
        author: manifest.author,
        signature_valid,
        status: status.as_str().to_string(),
        requested_capabilities: manifest.requested_capabilities,
        granted_capabilities: manifest.granted_capabilities,
    }
}

/// Lists every discovered plugin WITH its fail-closed verdict: verified
/// entries are registrable, the rest (`invalid` / `unavailable` /
/// `incompatible`) carry their signed manifest data for display and are
/// NEVER registered or executed. Entries without any parseable manifest
/// (unidentifiable garbage) are skipped — there is nothing truthful to show.
#[tauri::command]
pub fn list_plugins(
    library: State<'_, hexforge_plugin_host::store::PluginLibrary>,
) -> Vec<PluginManifestDto> {
    use hexforge_plugin_host::store::PluginStatus;
    library
        .discover()
        .into_iter()
        .filter_map(|entry| {
            let manifest = entry.manifest?;
            let sig_valid = entry.status == PluginStatus::Verified;
            Some(plugin_manifest_dto(manifest, sig_valid, entry.status))
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPluginRequest {
    pub wasm_path: String,
    pub manifest_path: String,
}

#[tauri::command]
pub fn install_plugin(
    req: InstallPluginRequest,
    state: State<'_, Arc<AppState>>,
    plugin_runtime: State<'_, Arc<hexforge_plugin_host::PluginRuntime>>,
    library: State<'_, hexforge_plugin_host::store::PluginLibrary>,
) -> HexForgeResult<PluginManifestDto> {
    use hexforge_plugin_host::store::PluginStatus;
    validate_fs_path(&req.wasm_path, "wasmPath")?;
    validate_fs_path(&req.manifest_path, "manifestPath")?;
    let wasm_bytes = std::fs::read(&req.wasm_path).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot read wasm '{}': {e}", req.wasm_path))
    })?;
    let manifest_bytes = std::fs::read(&req.manifest_path).map_err(|e| {
        HexForgeError::invalid_input(format!("cannot read manifest '{}': {e}", req.manifest_path))
    })?;
    let _manifest: hexforge_plugin_host::PluginManifest =
        serde_json::from_slice(&manifest_bytes)
            .map_err(|e| HexForgeError::invalid_input(format!("manifest JSON invalid: {e}")))?;
    // Signatures are REQUIRED (fail-closed): sidecar `<manifest>.sig` and
    // `<manifest>.pub` must exist next to the manifest. There is no unsigned
    // "developer mode" — sign locally with
    // `hexforge-cli plugin bind <manifest> <wasm>` then `plugin sign`.
    let sig_path = format!("{}.sig", req.manifest_path);
    let pub_path = format!("{}.pub", req.manifest_path);
    let signature_hex = std::fs::read_to_string(&sig_path).unwrap_or_default();
    let pubkey_hex = std::fs::read_to_string(&pub_path).unwrap_or_default();

    // Validate → verify → atomically persist into the app library, then
    // re-verify from disk. The returned instance proves the full
    // persist → rediscover loop; it survives app restarts via discovery.
    // A same-id reinstall is a deterministic atomic replace (new package is
    // fully verified before the old one moves; rollback on failure).
    let entry = library
        .install_package(
            &wasm_bytes,
            &manifest_bytes,
            signature_hex.trim(),
            pubkey_hex.trim(),
        )
        .map_err(|e| match e {
            hexforge_plugin_host::PluginError::InvalidSignature(_)
            | hexforge_plugin_host::PluginError::InvalidPublicKey(_)
            | hexforge_plugin_host::PluginError::ManifestParse(_)
            | hexforge_plugin_host::PluginError::InvalidManifest(_)
            | hexforge_plugin_host::PluginError::CapabilityDenied(_)
            | hexforge_plugin_host::PluginError::Incompatible(_) => {
                HexForgeError::invalid_input(format!("install refused: {e}"))
            }
            other => HexForgeError::internal(format!("install failed: {other}")),
        })?;
    let instance = entry.instance.clone().ok_or_else(|| {
        HexForgeError::internal("installed package failed re-verification (bug)".to_string())
    })?;

    // Register as Transform via PluginTransform wrapper
    let runtime = plugin_runtime.inner().clone();
    let transform: &'static dyn hexforge_core::Transform = runtime
        .as_transform(instance.clone())
        .map(|pt| {
            let leaked: Box<dyn hexforge_core::Transform> = Box::new(pt);
            Box::leak(leaked) as &'static dyn hexforge_core::Transform
        })
        .map_err(|e| HexForgeError::internal(format!("plugin transform creation failed: {e}")))?;
    state.register_plugin(transform).map_err(|e| {
        // The package IS persisted on disk; only the in-session registry
        // refused it (duplicate id). Fail closed with a restart remedy
        // instead of serving a stale transform silently.
        HexForgeError::internal(format!(
            "plugin installed on disk but not registered in this session ({e}); \
             restart the app to load it"
        ))
    })?;

    Ok(plugin_manifest_dto(
        instance.manifest,
        true,
        PluginStatus::Verified,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCapabilityRequest {
    pub plugin_id: String,
    pub capability: String,
}

/// Privileged capabilities (single source; mirrors
/// `PluginRuntime::is_privileged_cap` and `store::POLICY_CAPABILITIES`).
/// Grants persist in the installed package's `grants.json` — a versioned
/// LOCAL state file that only narrows the signed `requested` set and is
/// re-clamped on every discovery. The signed manifest itself is never
/// rewritten (that would invalidate its Ed25519 signature).
const VALID_CAPABILITIES: [&str; 3] = ["filesystem_read", "filesystem_write", "network"];

#[tauri::command]
pub fn grant_capability(
    req: GrantCapabilityRequest,
    library: State<'_, hexforge_plugin_host::store::PluginLibrary>,
) -> HexForgeResult<bool> {
    if req.plugin_id.trim().is_empty() {
        return Err(HexForgeError::invalid_parameter(
            "pluginId",
            "pluginId must not be empty",
        ));
    }
    if !VALID_CAPABILITIES.contains(&req.capability.as_str()) {
        return Err(HexForgeError::invalid_parameter(
            "capability",
            format!("unknown capability '{}'", req.capability),
        ));
    }
    let entry = library.get(&req.plugin_id).ok_or_else(|| {
        HexForgeError::invalid_parameter(
            "pluginId",
            format!(
                "unknown plugin '{}' (only installed packages accept persisted grants)",
                req.plugin_id
            ),
        )
    })?;
    let manifest = entry.manifest.ok_or_else(|| {
        HexForgeError::internal(format!(
            "plugin '{}' has no readable manifest; reinstall it",
            req.plugin_id
        ))
    })?;
    let mut next = manifest.granted_capabilities;
    if !next.iter().any(|c| c == &req.capability) {
        next.push(req.capability.clone());
    }
    library
        .set_grants(&req.plugin_id, &next)
        .map_err(|e| HexForgeError::internal(format!("grant refused: {e}")))?;
    Ok(true)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeCapabilityRequest {
    pub plugin_id: String,
    pub capability: String,
}

#[tauri::command]
pub fn revoke_capability(
    req: RevokeCapabilityRequest,
    library: State<'_, hexforge_plugin_host::store::PluginLibrary>,
) -> HexForgeResult<bool> {
    if req.plugin_id.trim().is_empty() {
        return Err(HexForgeError::invalid_parameter(
            "pluginId",
            "pluginId must not be empty",
        ));
    }
    if !VALID_CAPABILITIES.contains(&req.capability.as_str()) {
        return Err(HexForgeError::invalid_parameter(
            "capability",
            format!("unknown capability '{}'", req.capability),
        ));
    }
    let entry = library.get(&req.plugin_id).ok_or_else(|| {
        HexForgeError::invalid_parameter(
            "pluginId",
            format!(
                "unknown plugin '{}' (only installed packages accept persisted grants)",
                req.plugin_id
            ),
        )
    })?;
    let manifest = entry.manifest.ok_or_else(|| {
        HexForgeError::internal(format!(
            "plugin '{}' has no readable manifest; reinstall it",
            req.plugin_id
        ))
    })?;
    let next: Vec<String> = manifest
        .granted_capabilities
        .into_iter()
        .filter(|c| c != &req.capability)
        .collect();
    library
        .set_grants(&req.plugin_id, &next)
        .map_err(|e| HexForgeError::internal(format!("revoke refused: {e}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::graph::{NodeId, OperationNode};
    use hexforge_engine::error::HexForgeErrorKind;
    use hexforge_engine::graph_dto::OperationNodeDto;

    #[test]
    fn detect_mime_known_magic_bytes() {
        assert_eq!(
            detect_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).as_deref(),
            Some("image/png")
        );
        assert_eq!(
            detect_mime(&[0xFF, 0xD8, 0xFF, 0xE0]).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_mime(b"PK\x03\x04rest").as_deref(),
            Some("application/zip")
        );
        assert_eq!(
            detect_mime(&[0x1F, 0x8B, 0x08, 0x00]).as_deref(),
            Some("application/gzip")
        );
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

        assert_eq!(
            parse_handle("").unwrap_err().kind,
            HexForgeErrorKind::InvalidInput
        );
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
        let source_handle = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(literal.to_vec()));

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

        let output = scheduler::execute_chain(&state, &encode_id, &fresh_token(), &no_progress)
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
            assert_eq!(
                snaps[0].output_content_hash,
                Some(blake3::hash(&rot13_hello))
            );
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
        let source_handle = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(b"x".to_vec()));
        state.graph.write().insert_node(OperationNode {
            id: node_id,
            operation_id: "text.rot13".into(),
            operation_version: "9.9.9".into(),
            params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
            inputs: vec![],
        });

        let err =
            scheduler::execute_chain(&state, &node_id, &fresh_token(), &no_progress).unwrap_err();
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
        let source_handle = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(b"x".to_vec()));
        state.graph.write().insert_node(OperationNode {
            id: node_id,
            operation_id: "encoding.nonexistent".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": source_handle.to_string() }),
            inputs: vec![],
        });

        let err =
            scheduler::execute_chain(&state, &node_id, &fresh_token(), &no_progress).unwrap_err();
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
            origin: "builtin".into(),
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
                "origin": "builtin",
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
            input_content_hashes: None,
            input_snapshot_ids: vec![],
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
                "inputContentHashes": null,
                "inputSnapshotIds": [],
                "outputContentHash": "bb".repeat(32),
            })
        );
    }

    #[test]
    fn plugin_manifest_dto_matches_ts_contract() {
        let dto = PluginManifestDto {
            id: "plugin.example".into(),
            display_name: "Example".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            category: "Plugin".into(),
            author: "HexForge".into(),
            signature_valid: true,
            status: "verified".into(),
            requested_capabilities: vec!["filesystem_read".into()],
            granted_capabilities: vec![],
        };
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "id": "plugin.example",
                "displayName": "Example",
                "name": "Example",
                "version": "1.0.0",
                "category": "Plugin",
                "author": "HexForge",
                "signatureValid": true,
                "status": "verified",
                "requestedCapabilities": ["filesystem_read"],
                "grantedCapabilities": [],
            })
        );
    }

    #[test]
    fn install_plugin_request_matches_ts_contract() {
        let req = InstallPluginRequest {
            wasm_path: "plugin.wasm".into(),
            manifest_path: "manifest.json".into(),
        };
        assert_eq!(
            serde_json::to_value(&req).unwrap(),
            serde_json::json!({
                "wasmPath": "plugin.wasm",
                "manifestPath": "manifest.json",
            })
        );
        // Verify error handling for invalid path (empty) is tested via validate_fs_path
        assert!(validate_fs_path("", "wasmPath").is_err());
        assert!(validate_fs_path("a\0b", "wasmPath").is_err());
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
            origin: "builtin".into(),
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

        let resp = PatchSourceResponse {
            new_size_bytes: 8192,
        };
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
        GraphDto {
            nodes,
            required_plugins: Vec::new(),
        }
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

        export_recipe_inner(
            &state,
            ExportRecipeRequest {
                graph: dto.clone(),
                target_path: path.to_string_lossy().into_owned(),
            },
        )
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

        let path =
            std::env::temp_dir().join(format!("hexforge-recipe-miss-{}.json", Uuid::new_v4()));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&GraphDto {
                nodes,
                required_plugins: Vec::new(),
            })
            .unwrap(),
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
                graph: GraphDto {
                    nodes,
                    required_plugins: Vec::new(),
                },
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

        let bad_json = std::env::temp_dir().join(format!("hexforge-bad-{}.json", Uuid::new_v4()));
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
            serde_json::to_string_pretty(&GraphDto {
                nodes,
                required_plugins: Vec::new(),
            })
            .unwrap(),
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

#[cfg(test)]
mod plugin_recipe_tests {
    //! Recipe↔plugin contract at the Tauri command layer: palette origin,
    //! `requiredPlugins` export (auto-collected, deduplicated), import
    //! dependency checks (matching / missing / wrong version), and backward
    //! compatibility for pre-plugin recipe files.
    use super::*;
    use hexforge_core::graph::NodeId;
    use hexforge_engine::graph_dto::OperationNodeDto;

    struct FakePlugin {
        version: &'static str,
    }

    impl hexforge_core::Transform for FakePlugin {
        fn id(&self) -> &'static str {
            "plugin:test.op"
        }
        fn version(&self) -> &'static str {
            self.version
        }
        fn display_name(&self) -> &'static str {
            "Fake Plugin Op"
        }
        fn category(&self) -> &'static str {
            "Test"
        }
        fn capabilities(&self) -> hexforge_core::TransformCapabilities {
            hexforge_core::TransformCapabilities {
                deterministic: true,
                streamable: false,
                memory_cost: hexforge_core::MemoryCost::FullBuffer,
            }
        }
        fn origin(&self) -> &'static str {
            "plugin"
        }
        fn apply<'x>(
            &self,
            input: hexforge_core::transform::ByteView<'x>,
            _params: &serde_json::Value,
            _ctx: &dyn hexforge_core::transform::ExecutionContext,
        ) -> Result<hexforge_core::transform::ByteView<'x>, hexforge_core::TransformError> {
            Ok(input)
        }
    }

    fn state_with_plugin(version: &'static str) -> AppState {
        let state = AppState::new(hexforge_ops::build_registry());
        let leaked: &'static dyn hexforge_core::Transform =
            Box::leak(Box::new(FakePlugin { version }));
        state
            .register_plugin(leaked)
            .expect("canonical id registers");
        state
    }

    fn node_dto(id: &NodeId, op: &str, version: &str) -> (String, OperationNodeDto) {
        (
            id.to_string(),
            OperationNodeDto {
                id: id.to_string(),
                operation_id: op.into(),
                operation_version: version.into(),
                params: serde_json::json!({}),
                inputs: vec![],
            },
        )
    }

    fn tmp_file(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "hexforge-recipe-cmd-{}-{}.json",
            tag,
            NodeId::new_v4()
        ))
    }

    #[test]
    fn list_operations_marks_plugin_origin() {
        let state = state_with_plugin("2.0.0");
        let ops = list_operations_inner(&state);
        let plugin = ops.iter().find(|o| o.id == "plugin:test.op").unwrap();
        assert_eq!(plugin.origin, "plugin");
        let builtin = ops
            .iter()
            .find(|o| o.id == "encoding.base64.encode")
            .unwrap();
        assert_eq!(builtin.origin, "builtin");
    }

    #[test]
    fn export_collects_required_plugins_deduped() {
        let state = state_with_plugin("2.0.0");
        let (k1, n1) = node_dto(&NodeId::new_v4(), "plugin:test.op", "2.0.0");
        let (k2, n2) = node_dto(&NodeId::new_v4(), "plugin:test.op", "2.0.0");
        let (k3, n3) = node_dto(&NodeId::new_v4(), "encoding.base64.encode", "1.0.0");
        let dto = GraphDto {
            nodes: [(k1, n1), (k2, n2), (k3, n3)].into_iter().collect(),
            required_plugins: Vec::new(),
        };
        let path = tmp_file("export");
        export_recipe_inner(
            &state,
            ExportRecipeRequest {
                graph: dto,
                target_path: path.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        // Auto-collected server-side, deduplicated (one entry, not two).
        assert_eq!(
            parsed["requiredPlugins"],
            serde_json::json!([{ "id": "plugin:test.op", "version": "2.0.0" }])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_builtin_only_graph_has_no_required_plugins() {
        let state = AppState::new(hexforge_ops::build_registry());
        let (k1, n1) = node_dto(&NodeId::new_v4(), "encoding.base64.encode", "1.0.0");
        let dto = GraphDto {
            nodes: [(k1, n1)].into_iter().collect(),
            required_plugins: Vec::new(),
        };
        let path = tmp_file("export-builtin");
        export_recipe_inner(
            &state,
            ExportRecipeRequest {
                graph: dto,
                target_path: path.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["requiredPlugins"], serde_json::json!([]));
        let _ = std::fs::remove_file(&path);
    }

    fn recipe_file_with_dep(
        nodes: std::collections::HashMap<String, OperationNodeDto>,
        dep: serde_json::Value,
    ) -> std::path::PathBuf {
        let recipe = serde_json::json!({
            "nodes": nodes,
            "requiredPlugins": [dep],
        });
        let path = tmp_file("import");
        std::fs::write(&path, serde_json::to_string(&recipe).unwrap()).unwrap();
        path
    }

    fn import_dep(state: &AppState, version: &str) -> ImportRecipeResponse {
        let (k1, n1) = node_dto(&NodeId::new_v4(), "plugin:test.op", version);
        let path = recipe_file_with_dep(
            [(k1, n1)].into_iter().collect(),
            serde_json::json!({ "id": "plugin:test.op", "version": version }),
        );
        let resp = import_recipe_inner(
            state,
            ImportRecipeRequest {
                source_path: path.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let _ = std::fs::remove_file(&path);
        resp
    }

    #[test]
    fn import_with_installed_matching_plugin_is_clean() {
        let state = state_with_plugin("2.0.0");
        let resp = import_dep(&state, "2.0.0");
        assert!(
            resp.missing_plugins.is_empty(),
            "{:?}",
            resp.missing_plugins
        );
        assert!(resp.missing_operations.is_empty());
    }

    #[test]
    fn import_with_missing_plugin_is_reported_not_hidden() {
        let state = AppState::new(hexforge_ops::build_registry());
        let resp = import_dep(&state, "2.0.0");
        assert_eq!(resp.missing_plugins.len(), 1);
        assert_eq!(resp.missing_plugins[0].id, "plugin:test.op");
        assert_eq!(resp.missing_plugins[0].version, "2.0.0");
        assert!(resp.missing_plugins[0].reason.contains("not installed"));
    }

    #[test]
    fn import_with_wrong_plugin_version_is_reported() {
        let state = state_with_plugin("9.9.9");
        let resp = import_dep(&state, "2.0.0");
        assert_eq!(resp.missing_plugins.len(), 1);
        assert!(resp.missing_plugins[0].reason.contains("version mismatch"));
    }

    #[test]
    fn import_old_recipe_without_required_plugins_stays_compatible() {
        let state = AppState::new(hexforge_ops::build_registry());
        let (k1, n1) = node_dto(&NodeId::new_v4(), "encoding.base64.encode", "1.0.0");
        let nodes: std::collections::HashMap<String, OperationNodeDto> =
            [(k1, n1)].into_iter().collect();
        let recipe = serde_json::json!({ "nodes": nodes });
        let path = tmp_file("import-old");
        std::fs::write(&path, serde_json::to_string(&recipe).unwrap()).unwrap();
        let resp = import_recipe_inner(
            &state,
            ImportRecipeRequest {
                source_path: path.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        assert!(resp.missing_plugins.is_empty());
        assert!(resp.missing_operations.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    // ===== CyberChef export tests (FR-7.2) =====

    fn cyberchef_node_dto(
        id: &NodeId,
        op: &str,
        inputs: Vec<String>,
    ) -> (String, OperationNodeDto) {
        (
            id.to_string(),
            OperationNodeDto {
                id: id.to_string(),
                operation_id: op.into(),
                operation_version: "1.0.0".into(),
                params: serde_json::json!({}),
                inputs,
            },
        )
    }

    fn cyberchef_export(
        state: &AppState,
        nodes: std::collections::HashMap<String, OperationNodeDto>,
        target_path: std::path::PathBuf,
    ) -> HexForgeResult<ExportCyberChefRecipeResponse> {
        cyberchef_export_inner(
            ExportCyberChefRecipeRequest {
                graph: GraphDto {
                    nodes,
                    required_plugins: Vec::new(),
                },
                target_path: target_path.to_string_lossy().into_owned(),
            },
            state,
        )
    }

    #[test]
    fn export_linear_chain_of_builtin_ops() {
        let state = AppState::new(hexforge_ops::build_registry());
        let s = NodeId::new_v4();
        let a = NodeId::new_v4();
        let b = NodeId::new_v4();
        let (k_s, n_s) = cyberchef_node_dto(&s, "encoding.base64.encode", vec![]);
        let (k_a, n_a) = cyberchef_node_dto(&a, "encoding.hex.encode", vec![k_s.clone()]);
        let (k_b, n_b) = cyberchef_node_dto(&b, "text.rot13", vec![k_a.clone()]);
        let path = tmp_file("cc-export-linear");
        let resp = cyberchef_export(
            &state,
            [(k_s, n_s), (k_a, n_a), (k_b, n_b)].into_iter().collect(),
            path.clone(),
        )
        .unwrap();
        assert!(resp.warnings.is_empty(), "warnings: {:?}", resp.warnings);
        let parsed: serde_json::Value = serde_json::from_str(&resp.content).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["op"], "To Base64");
        assert_eq!(arr[1]["op"], "To Hex");
        assert_eq!(arr[2]["op"], "ROT13");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_fork_graph_is_rejected() {
        let state = AppState::new(hexforge_ops::build_registry());
        let s = NodeId::new_v4();
        let f = NodeId::new_v4();
        let a = NodeId::new_v4();
        let b = NodeId::new_v4();
        let (k_s, n_s) = cyberchef_node_dto(&s, "input.file", vec![]);
        let (k_f, n_f) = cyberchef_node_dto(&f, "encoding.base64.encode", vec![k_s.clone()]);
        let (k_a, n_a) = cyberchef_node_dto(&a, "encoding.hex.encode", vec![k_f.clone()]);
        let (k_b, n_b) = cyberchef_node_dto(&b, "text.rot13", vec![k_f.clone()]);
        let path = tmp_file("cc-export-fork");
        let err = cyberchef_export(
            &state,
            [(k_s, n_s), (k_f, n_f), (k_a, n_a), (k_b, n_b)]
                .into_iter()
                .collect(),
            path.clone(),
        )
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(msg.contains("fork"), "expected fork error, got: {msg}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_merge_graph_is_rejected() {
        let state = AppState::new(hexforge_ops::build_registry());
        let s = NodeId::new_v4();
        let a = NodeId::new_v4();
        let m = NodeId::new_v4();
        let (k_s, n_s) = cyberchef_node_dto(&s, "input.file", vec![]);
        let (k_a, n_a) = cyberchef_node_dto(&a, "encoding.base64.encode", vec![k_s.clone()]);
        let (k_m, n_m) =
            cyberchef_node_dto(&m, "encoding.hex.encode", vec![k_s.clone(), k_a.clone()]);
        let path = tmp_file("cc-export-merge");
        let err = cyberchef_export(
            &state,
            [(k_s, n_s), (k_a, n_a), (k_m, n_m)].into_iter().collect(),
            path.clone(),
        )
        .unwrap_err();
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("inputs") || msg.contains("merge"),
            "expected merge error, got: {msg}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_unsupported_op_yields_warning() {
        let state = AppState::new(hexforge_ops::build_registry());
        let s = NodeId::new_v4();
        let a = NodeId::new_v4();
        let (k_s, n_s) = cyberchef_node_dto(&s, "encoding.base64.encode", vec![]);
        let (k_a, n_a) = cyberchef_node_dto(&a, "crypto.aes.encrypt", vec![k_s.clone()]);
        let path = tmp_file("cc-export-warn");
        let resp = cyberchef_export(
            &state,
            [(k_s, n_s), (k_a, n_a)].into_iter().collect(),
            path.clone(),
        )
        .unwrap();
        assert_eq!(resp.warnings.len(), 1);
        assert!(resp.warnings[0].contains("crypto.aes.encrypt"));
        let parsed: serde_json::Value = serde_json::from_str(&resp.content).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["op"], "To Base64");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_empty_graph_is_rejected() {
        let state = AppState::new(hexforge_ops::build_registry());
        let path = tmp_file("cc-export-empty");
        let err =
            cyberchef_export(&state, std::collections::HashMap::new(), path.clone()).unwrap_err();
        let msg = format!("{:?}", err);
        assert!(msg.contains("source"), "expected source error, got: {msg}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn export_output_writes_source_to_disk() {
        let state = AppState::new(hexforge_ops::build_registry());
        let handle = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(b"Hello, FR-5.4!".to_vec()));

        let temp = std::env::temp_dir().join("hexforge_export_test.bin");
        let path_str = temp.to_str().unwrap().to_string();

        let resp = export_output_inner(
            &state,
            ExportOutputRequest {
                handle: handle.to_string(),
                target_path: path_str.clone(),
            },
        )
        .unwrap();

        assert_eq!(resp.bytes_written, 14);
        let written = std::fs::read(&temp).unwrap();
        assert_eq!(written, b"Hello, FR-5.4!");
        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn export_output_unknown_handle_errors() {
        let state = AppState::new(hexforge_ops::build_registry());
        let result = export_output_inner(
            &state,
            ExportOutputRequest {
                handle: "nonexistent".into(),
                target_path: "out.bin".into(),
            },
        );
        assert!(result.is_err());
    }
}
