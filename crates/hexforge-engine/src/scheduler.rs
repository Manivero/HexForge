//! Планировщик исполнения цепочки узлов (MVP-ядро `hexforge-stream`,
//! живёт в hexforge-engine: ему нужен домен — реестр, кэш, история; см.
//! docs/04 §6 про размещение).
//!
//! Семантики, реализуемые поверх чистого исполнения:
//! - **Chunked streaming**: streamable-операции (`capabilities().streamable`)
//!   исполняются чанками через `apply_chunk` над zero-copy срезами входного
//!   буфера; выход накапливается аккумулятором. Cross-node pipelining и
//!   bounded backpressure (mpsc) — следующий шаг (см. docs/04 §6).
//! - **Memoization**: content-addressed LRU-кэш по
//!   `reproducibility_key(op@ver :: input_hash :: params)`; хэш входа считается
//!   один раз и переиспользуется для записи снапшота истории.
//! - **Cooperative cancellation**: токен опрашивается между узлами и между
//!   чанками streamable-узлов. Не-стримовая `apply` непрерываема до возврата —
//!   задокументированное ограничение до внедрения ctx-чеков в самих операциях.
//! - **Merge**: узел с N>1 входами требует merge-реализацию операции
//!   (`TransformRegistry::get_merge`, PRD FR-1.2/FR-1.4).

use crate::error::{HexForgeError, HexForgeResult};
use crate::state::{AppState, CancellationToken};
use hexforge_core::graph::{NodeId, OperationNode};
use hexforge_core::transform::{ExecutionContext, NullExecutionContext};
use hexforge_stream::{chunk_ranges, DEFAULT_CHUNK_SIZE_BYTES};
use serde::Serialize;
use std::borrow::Cow;
use parking_lot::Mutex as StageMutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use uuid::Uuid;

/// Событие прогресса выполнения (`op://progress`, 05-IPC-CONTRACT.md §events).
/// Поле `bytesTotal: null` соответствует TS `number | null`. Доставку
/// получателю выбирает хост: GUI эмитит в WebView, CLI может печатать.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub node_id: String,
    pub bytes_processed: u64,
    pub bytes_total: Option<u64>,
}

/// Параллельный конвейер включается для прогонов с ≥2 стадиями и входом
/// больше этого порога; иначе — последовательный путь (без накладных
/// расходов на потоки для мелких данных).
const PARALLEL_PIPELINE_MIN_BYTES: usize = DEFAULT_CHUNK_SIZE_BYTES;

/// Ёмкость bounded-канала между стадиями: backpressure ограничивает память
/// величиной stages × capacity × chunk_size (docs/04 §6).
const PIPELINE_CHANNEL_CAPACITY: usize = 4;

/// Получатель событий прогресса планировщика (GUI: emit в WebView,
/// CLI: stderr/ничего). Ошибки доставки — забота реализации колбэка.
pub type ProgressSink<'a> = &'a dyn Fn(&ProgressEvent);

/// Контекст выполнения одного узла для встроенных операций: кооперативная
/// отмена + заглушка прогресса (события на границах узлов эмитит сам
/// планировщик; встроенные MVP-операции per-chunk прогресс не репортят).
struct RunContext {
    token: CancellationToken,
}

impl ExecutionContext for RunContext {
    fn report_progress(&self, _bytes_processed: u64, _bytes_total: Option<u64>) {}

    fn is_cancelled(&self) -> bool {
        self.token.load(Ordering::Relaxed)
    }
}

/// Исполняет входную цепочку запрошенного узла и возвращает его выход.
/// Результат разделён через `Arc`: он попадает и в SourceStore (для превью),
/// и в кэш промежуточных узлов без копий.
pub fn execute_chain(
    state: &AppState,
    root: &NodeId,
    token: &CancellationToken,
    on_progress: ProgressSink<'_>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    resolve_node(root, state, token, on_progress)
}

fn check_cancelled(node_id: &NodeId, token: &CancellationToken) -> HexForgeResult<()> {
    if token.load(Ordering::Relaxed) {
        return Err(HexForgeError::cancelled_for_node(
            node_id,
            format!("execution of node {node_id} was cancelled"),
        ));
    }
    Ok(())
}

fn resolve_node(
    node_id: &NodeId,
    state: &AppState,
    token: &CancellationToken,
    on_progress: ProgressSink<'_>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    check_cancelled(node_id, token)?;

    let node = {
        let graph = state.graph.read();
        graph.nodes.get(node_id).cloned().ok_or_else(|| {
            HexForgeError::internal_for_node(node_id, format!("node {node_id} not found in current graph"))
        })?
    };

    // Cross-node streaming (docs/04 §6): streamable single-input узел
    // пытается втянуть вверх по цепочке максимальный стримовый прогон —
    // промежуточные выходы живут размером с чанк, полный буфер только у
    // последней стадии. Всё остальное (merge, non-streamable, корни)
    // исполняется универсальным путём ниже.
    if node.inputs.len() == 1 {
        if let Some(fusion) = build_stream_fusion(state, &node)? {
            return execute_fused_run(state, fusion, token, on_progress);
        }
    }

    // Входы: 0 — источник из SourceStore, 1 — рекурсивный выход родителя,
    // N — merge-ветка (порядок inputs — часть контракта операции).
    let inputs: Vec<Arc<Vec<u8>>> = match node.inputs.len() {
        0 => vec![resolve_source_input(&node, node_id, state)?],
        1 => {
            let parent_output = resolve_node(&node.inputs[0], state, token, on_progress)?;
            check_cancelled(node_id, token)?;
            vec![parent_output]
        }
        _ => {
            let mut resolved = Vec::with_capacity(node.inputs.len());
            for input_id in &node.inputs {
                resolved.push(resolve_node(input_id, state, token, on_progress)?);
                check_cancelled(node_id, token)?;
            }
            resolved
        }
    };

    // Memoization: хэш первичного буфера входа считается один раз и идёт
    // и в ключ кэша, и в snapshot истории.
    let primary_input_hash = blake3::hash(&inputs[0]);
    let cache_key = hexforge_core::reproducibility_key(
        &node.operation_id,
        &node.operation_version,
        &primary_input_hash.to_hex()[..],
        &node.params,
    );

    if let Some(cached) = state.cache.lock().get(&cache_key) {
        // Cache-hit НЕ пишет новый снапшот: история — граф СОСТОЯНИЙ
        // (FR-4.2), а состояние узла не изменилось; дубль засорял бы DAG
        // нулевыми узлами и смещал голову при прыжках.
        // История — граф состояний: повтор идентичного запуска переиспользует
        // существующий снапшот (head остаётся), не создавая дубль.
        record_output(&node, state, primary_input_hash, blake3::hash(&cached));
        emit_progress(on_progress, node_id, cached.len());
        return Ok(cached);
    }

    let output: Arc<Vec<u8>> = if node.inputs.len() > 1 {
        execute_merge_node(&node, state, token, inputs)?
    } else {
        execute_unary_node(&node, state, token, Arc::clone(&inputs[0]))?
    };

    state.cache.lock().put(cache_key, Arc::clone(&output));
    emit_progress(on_progress, node_id, output.len());
    record_output(&node, state, primary_input_hash, blake3::hash(&output));

    Ok(output)
}

/// Байты корневого узла: SourceStore по `params.sourceHandle`.
/// NOTE: `.to_vec()` копирует буфер источника (задокументированный tech debt,
/// см. комментарий в commands.rs до переноса сюда); mmap-view внутри кэша —
/// следующий шаг после перехода SourceEntry на Arc-семантику.
fn resolve_source_input(
    node: &OperationNode,
    node_id: &NodeId,
    state: &AppState,
) -> HexForgeResult<Arc<Vec<u8>>> {
    let handle_str = node
        .params
        .get("sourceHandle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            HexForgeError::invalid_parameter("sourceHandle", "root node requires params.sourceHandle")
        })?;
    let handle =
        Uuid::parse_str(handle_str).map_err(|_| HexForgeError::invalid_input(format!("'{handle_str}' is not a valid source handle")))?;
    let sources = state.sources.read();
    let entry = sources.get(&handle).ok_or_else(|| {
        HexForgeError::internal_for_node(node_id, format!("unknown source handle: {handle_str}"))
    })?;
    Ok(Arc::new(entry.as_bytes().to_vec()))
}

fn lookup_transform(
    node: &OperationNode,
    state: &AppState,
) -> HexForgeResult<&'static dyn hexforge_core::Transform> {
    let transform = state
        .registry
        .get(&node.operation_id)
        .ok_or_else(|| {
            HexForgeError::internal_for_node(
                node.id,
                format!("unknown operation: {}", node.operation_id),
            )
        })?;
    if transform.version() != node.operation_version {
        return Err(HexForgeError::internal_for_node(
            node.id,
            format!(
                "operation '{}' version mismatch: node expects {}, registry has {} \
                 (reproducibility guarantee violated, see FR-4.2)",
                node.operation_id,
                node.operation_version,
                transform.version()
            ),
        ));
    }
    Ok(transform)
}

/// Унарный узел: chunked-путь для `streamable`, полный буфер иначе.
fn execute_unary_node(
    node: &OperationNode,
    state: &AppState,
    token: &CancellationToken,
    input: Arc<Vec<u8>>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    let transform = lookup_transform(node, state)?;
    let caps = transform.capabilities();

    if !caps.streamable {
        // Токен отмены обязан доходить и до не-стримовых операций:
        // долгая apply() опрашивает ctx.is_cancelled() на своих чекпоинтах.
        let ctx: &dyn ExecutionContext = &RunContext {
            token: Arc::clone(token),
        };
        let result = transform.apply(Cow::Borrowed(input.as_slice()), &node.params, ctx);
        return match result {
            Ok(view) => Ok(Arc::new(view.into_owned())),
            Err(e) => Err(HexForgeError::from(e)),
        };
    }

    // Chunked-путь: zero-copy срезы входа, аккумуляция выхода. Пустой вход —
    // один вызов apply_chunk с пустым срезом и is_last=true (контракт трейта).
    // `chunk_state` — per-node состояние между чанками, живёт весь цикл
    // (контракт docs/04 §6: заводится и освобождается планировщиком).
    let ranges = chunk_ranges(input.len(), hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES);
    let effective_ranges: &[(usize, usize)] = if ranges.is_empty() {
        &[(0, 0)]
    } else {
        &ranges
    };
    let mut chunk_state: Box<dyn std::any::Any + Send> = Box::new(());
    let mut accumulated: Vec<u8> = Vec::new();
    for (index, (start, end)) in effective_ranges.iter().enumerate() {
        check_cancelled(&node.id, token)?;
        let piece = transform
            .apply_chunk(
                &input[*start..*end],
                index + 1 == effective_ranges.len(),
                &mut chunk_state,
                &node.params,
                &RunContext {
                    token: Arc::clone(token),
                },
            )
            .map_err(HexForgeError::from)?;
        accumulated.extend_from_slice(&piece);
    }
    Ok(Arc::new(accumulated))
}

/// Merge-узел: операция обязана реализовать MergeTransform.
fn execute_merge_node(
    node: &OperationNode,
    state: &AppState,
    token: &CancellationToken,
    inputs: Vec<Arc<Vec<u8>>>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    let merge = state.registry.get_merge(&node.operation_id).ok_or_else(|| {
        HexForgeError::invalid_input(format!(
            "operation '{}' does not support {} inputs: node requires a merge operation \
             (PRD FR-1.4), but no MergeTransform is registered",
            node.operation_id,
            node.inputs.len()
        ))
    })?;
    // Версию проверяем по базовой Transform-регистрации (merge-трейт её
    // наследует; обе карты заполняются одной операцией).
    lookup_transform(node, state)?;

    let views: Vec<Cow<[u8]>> = inputs.iter().map(|v| Cow::Borrowed(v.as_slice())).collect();
    let result = merge.apply_merge(
        views,
        &node.params,
        &RunContext {
            token: Arc::clone(token),
        },
    );
    match result {
        Ok(view) => Ok(Arc::new(view.into_owned())),
        Err(e) => Err(HexForgeError::from(e)),
    }
}

// ---------- Cross-node streaming fusion (docs/04 §6) ----------

/// Одна стадия слитного стримового прогона.
struct FusionStage {
    node: OperationNode,
    transform: &'static dyn hexforge_core::Transform,
    input_hasher: blake3::Hasher,
    output_hasher: blake3::Hasher,
    state: Box<dyn std::any::Any + Send>,
}

/// Собирает максимальный прогон streamable single-input узлов, заканчивающийся
/// в `node`. Возвращает стадии снизу-вверх и идентификатор базы (узел, чей
/// выход материализуется обычным путём), либо `None`, если узел сам не
/// streamable. Границы прогона: корень-источник, non-streamable операция,
/// merge-узел или отсутствующий родитель.
fn build_stream_fusion(state: &AppState, node: &OperationNode) -> HexForgeResult<Option<FusedRun>> {
    let top = lookup_transform(node, state)?;
    if !top.capabilities().streamable {
        return Ok(None);
    }

    let mut stages: Vec<Arc<StageMutex<FusionStage>>> = vec![Arc::new(StageMutex::new(FusionStage {
        node: node.clone(),
        transform: top,
        input_hasher: blake3::Hasher::new(),
        output_hasher: blake3::Hasher::new(),
        state: Box::new(()),
    }))];

    let mut cursor = node.clone();
    let base_parent: Option<NodeId> = loop {
        if cursor.inputs.len() != 1 {
            break None; // база — сам источник (root)
        }
        let parent_id = cursor.inputs[0];
        let parent = {
            let graph = state.graph.read();
            graph.nodes.get(&parent_id).cloned()
        };
        let Some(parent) = parent else {
            break Some(parent_id); // висячий вход — пусть общий путь отрапортует
        };
        let p_transform = lookup_transform(&parent, state)?;
        if !p_transform.capabilities().streamable {
            // Родитель не-streamable: его полный выход станет входом прогона.
            break Some(parent_id);
        }
        stages.push(Arc::new(StageMutex::new(FusionStage {
            node: parent.clone(),
            transform: p_transform,
            input_hasher: blake3::Hasher::new(),
            output_hasher: blake3::Hasher::new(),
            state: Box::new(()),
        })));
        cursor = parent;
    };

    stages.reverse(); // снизу-вверх: первая стадия потребляет базовый буфер
    Ok(Some(FusedRun { stages, base_parent }))
}

struct FusedRun {
    stages: Vec<Arc<StageMutex<FusionStage>>>,
    /// Родитель первой стадии; `None` — база является source-root.
    base_parent: Option<NodeId>,
}

/// Исполняет слитный прогон одним вложенным чанк-циклом:
/// внешний итератор — чанки базового буфера, внутренний — стадии.
/// Промежуточные выходы живут размером с чанк; полные буферы — только у
/// последней стадии (результат) и у базового входа.
///
/// Memoization внутри прогона: per-stage выходы не материализуются, поэтому
/// кэшируется ТОЛЬКО финальная стадия; снапшоты истории пишутся за каждую
/// стадию по инкрементальным blake3-хэшам границ (FR-4.2 без буферизации).
/// Диспетчер слитного прогона: разрешает базовый вход и выбирает стратегию —
/// параллельный конвейер (≥2 стадий и большой вход) либо последовательный
/// цикл (малые данные/одна стадия: без накладных расходов на потоки).
fn execute_fused_run(
    state: &AppState,
    run: FusedRun,
    token: &CancellationToken,
    on_progress: ProgressSink<'_>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    let first_stage_id = { run.stages[0].lock().node.id };
    check_cancelled(&first_stage_id, token)?;

    // Базовый вход: выход родителя первой стадии либо байты источника.
    let base_input: Arc<Vec<u8>> = match run.base_parent {
        Some(parent_id) => resolve_node(&parent_id, state, token, on_progress)?,
        None => {
            let first = { run.stages[0].lock().node.clone() };
            resolve_source_input(&first, &first.id, state)?
        }
    };

    if run.stages.len() >= 2 && base_input.len() > PARALLEL_PIPELINE_MIN_BYTES {
        execute_fused_parallel(state, run, base_input, token, on_progress)
    } else {
        execute_fused_sequential(state, run, base_input, token, on_progress)
    }
}

/// Последовательный вариант fusion: один поток, чанк проходит все стадии
/// подряд; промежуточные буферы размером с чанк.
fn execute_fused_sequential(
    state: &AppState,
    run: FusedRun,
    base_input: Arc<Vec<u8>>,
    token: &CancellationToken,
    on_progress: ProgressSink<'_>,
) -> HexForgeResult<Arc<Vec<u8>>> {

    let ranges = chunk_ranges(base_input.len(), hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES);
    let effective_ranges: &[(usize, usize)] = if ranges.is_empty() {
        &[(0, 0)]
    } else {
        &ranges
    };

    let mut final_output: Vec<u8> = Vec::with_capacity(base_input.len());
    let ctx_run = RunContext {
        token: Arc::clone(token),
    };

    for (index, (start, end)) in effective_ranges.iter().copied().enumerate() {
        let last_stage_id = { run.stages[run.stages.len() - 1].lock().node.id };
        check_cancelled(&last_stage_id, token)?;
        let is_last_chunk = index + 1 == effective_ranges.len();

        let mut piece: Vec<u8> = base_input[start..end].to_vec();
        for stage in run.stages.iter() {
            // ВАЖНО: guard ограничен блоком (урок про read-guard/if-let).
            {
                let mut st = stage.lock();
                st.input_hasher.update(piece.as_slice());
                // Клон параметров разделяет займы state/node (мелочь на фоне чанка).
                let params = st.node.params.clone();
                piece = st.transform.apply_chunk(
                    piece.as_slice(),
                    is_last_chunk,
                    &mut st.state,
                    &params,
                    &ctx_run,
                )?;
                st.output_hasher.update(piece.as_slice());
            }
        }
        final_output.extend_from_slice(piece.as_slice());
    }

    let out = finalize_fused(state, &run.stages, final_output);
    let last_stage_id = { run.stages[run.stages.len() - 1].lock().node.id };
    emit_progress(on_progress, &last_stage_id, out.len());
    Ok(out)
}

/// Общая финализация прогона (последовательного и параллельного):
/// снапшот на каждую стадию по инкрементальным хэшам + кэш только для
/// последней стадии.
fn finalize_fused(
    state: &AppState,
    stages: &[Arc<StageMutex<FusionStage>>],
    final_output: Vec<u8>,
) -> Arc<Vec<u8>> {
    let last_index = stages.len() - 1;
    let mut final_cached: Option<(String, Arc<Vec<u8>>)> = None;
    for (i, stage) in stages.iter().enumerate() {
        let st = stage.lock();
        let input_hash = st.input_hasher.finalize();
        let output_hash = st.output_hasher.finalize();
        record_output(&st.node, state, input_hash, output_hash);

        if i == last_index {
            let key = hexforge_core::reproducibility_key(
                &st.node.operation_id,
                &st.node.operation_version,
                &input_hash.to_hex()[..],
                &st.node.params,
            );
            let arc = Arc::new(final_output.clone());
            state.cache.lock().put(key.clone(), Arc::clone(&arc));
            final_cached = Some((key, arc));
        }
    }
    final_cached.expect("last stage always sets output").1
}

/// Параллельный конвейер: каждая стадия — отдельный поток, чанки идут через
/// bounded `sync_channel(cap)` → память ограничена stages × cap × chunk_size,
/// pull потребителя даёт backpressure. Ошибки операций и отмена передаются
 /// Err-сообщением вниз по цепочке; выход воркера роняет его sender и каскадно
/// закрывает апстрим.
fn execute_fused_parallel(
    state: &AppState,
    run: FusedRun,
    base_input: Arc<Vec<u8>>,
    token: &CancellationToken,
    on_progress: ProgressSink<'_>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    use std::sync::mpsc;

    type ChunkMsg = Result<(Vec<u8>, bool), HexForgeError>;

    let stages_len = run.stages.len();
    let mut txs: Vec<Option<mpsc::SyncSender<ChunkMsg>>> =
        Vec::with_capacity(stages_len + 1);
    let mut rxs: Vec<Option<mpsc::Receiver<ChunkMsg>>> =
        Vec::with_capacity(stages_len + 1);
    for _ in 0..=stages_len {
        let (tx, rx) = mpsc::sync_channel::<ChunkMsg>(PIPELINE_CHANNEL_CAPACITY);
        txs.push(Some(tx));
        rxs.push(Some(rx));
    }

    // Воркер на стадию: состояние/хэшеры стадии живут только в этом потоке.
    let mut handles = Vec::with_capacity(stages_len);
    for (i, stage_arc) in run.stages.iter().enumerate() {
        let stage = Arc::clone(stage_arc);
        let rx = rxs[i]
            .take()
            .expect("each stage consumes its own receiver once");
        let tx = txs[i + 1]
            .take()
            .expect("each stage owns its out-sender once");
        let token = Arc::clone(token);
        handles.push(std::thread::spawn(move || {
            let ctx = RunContext { token };
            loop {
                // Воркер НЕ инжектирует собственную отмену: сигнал обязаны
                // видеть операции через ctx (иначе гонка «кто первым увидел»
                // подменяет ошибку операции на безликий Cancelled).
                match rx.recv() {
                    Ok(Ok((chunk, is_last))) => {
                        let Some(mut st) = stage.try_lock() else {
                            let _ = tx.send(Err(HexForgeError::internal(
                                "pipeline stage lock unavailable",
                            )));
                            return;
                        };
                        st.input_hasher.update(chunk.as_slice());
                        let params = st.node.params.clone();
                        let applied = st.transform.apply_chunk(
                            chunk.as_slice(),
                            is_last,
                            &mut st.state,
                            &params,
                            &ctx,
                        );
                        match applied {
                            Ok(piece) => {
                                st.output_hasher.update(piece.as_slice());
                                drop(st);
                                if tx.send(Ok((piece, is_last))).is_err() {
                                    return; // потребитель завершился — shutdown
                                }
                                if is_last {
                                    return;
                                }
                            }
                            Err(te) => {
                                drop(st);
                                let _ = tx.send(Err(HexForgeError::from(te)));
                                return;
                            }
                        }
                    },
                    Ok(Err(e)) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                    Err(_) => return, // апстрим закрылся — shutdown
                }
            }
        }));
    }

    // Main: кормит базовые чанки в первую стадию и собирает выход последней.
    drop(rxs[0].take());
    let ranges = chunk_ranges(base_input.len(), hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES);
    let effective_ranges: &[(usize, usize)] = if ranges.is_empty() {
        &[(0, 0)]
    } else {
        &ranges
    };
    'feed: for (index, (start, end)) in effective_ranges.iter().copied().enumerate() {
        if token.load(Ordering::Relaxed) {
            break 'feed;
        }
        let msg: ChunkMsg = Ok((
            base_input[start..end].to_vec(),
            index + 1 == effective_ranges.len(),
        ));
        let first_tx = txs[0]
            .as_ref()
            .expect("main feed sender taken once");
        if first_tx.send(msg).is_err() {
            break 'feed;
        }
    }
    // вход исчерпан: первая стадия доработает и закроет цепочку.
    // (take() вместо drop: SyncSender не Copy — забираем по значению.)
    let _ = txs[0].take();

    let mut final_output: Vec<u8> = Vec::new();
    let mut pipe_err: Option<HexForgeError> = None;
    let final_rx = rxs[stages_len]
        .take()
        .expect("final receiver taken once");
    while let Ok(msg) = final_rx.recv() {
        match msg {
            Ok((piece, is_last)) => {
                final_output.extend_from_slice(piece.as_slice());
                if is_last {
                    break;
                }
            }
            Err(e) => {
                pipe_err = Some(e);
                break;
            }
        }
    }

    // Закрыть все каналы: воркеры дренируют/завершаются, join гарантирует,
    // что хэшеры стадий долиты до финализации истории.
    drop(txs);
    drop(rxs);
    for handle in handles {
        let _ = handle.join();
    }

    if let Some(e) = pipe_err {
        return Err(e);
    }

    let out = finalize_fused(state, &run.stages, final_output);
    let last_stage_id = { run.stages[stages_len - 1].lock().node.id };
    emit_progress(on_progress, &last_stage_id, out.len());
    Ok(out)
}


/// Событие инвалидации (`graph://invalidated`, 05-IPC-CONTRACT.md §events):
/// id узлов, чей кэшированный результат устарел после изменения графа.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphInvalidatedEvent {
    pub stale_node_ids: Vec<String>,
}

/// Чистый расчёт stale-набора при замене старого графа новым: узел изменился,
/// если он новый или у него сменились operation@version/params/inputs.
/// Stale = сам изменённый ∪ всё достижимое из него по исходящим рёбрам
/// НОВОГО графа (FR-1.6: точечная инвалидация без пересчёта всего графа).
pub fn compute_invalidated(
    old: &hexforge_core::Graph,
    new: &hexforge_core::Graph,
) -> Vec<String> {
    fn fingerprint(n: &hexforge_core::OperationNode) -> String {
        format!(
            "{}@{}::{:?}::{:?}",
            n.operation_id, n.operation_version, n.params, n.inputs
        )
    }

    let old_fp: std::collections::HashMap<NodeId, String> =
        old.nodes.iter().map(|(id, n)| (*id, fingerprint(n))).collect();

    let mut changed: Vec<NodeId> = Vec::new();
    for (id, n) in new.nodes.iter() {
        if old_fp.get(id).map(|f| f.as_str()) != Some(fingerprint(n).as_str()) {
            changed.push(*id);
        }
    }
    if changed.is_empty() {
        return Vec::new();
    }

    // Объединение downstream всех изменённых узлов; BTreeSet даёт
    // детерминированный порядок на проводе (HashMap-порядок не течёт).
    let mut stale: std::collections::BTreeSet<String> = Default::default();
    for c in changed {
        for id in new.downstream_of(c) {
            stale.insert(id.to_string());
        }
    }
    stale.into_iter().collect()
}

/// Stale-набор при мутации ИСТОЧНИКА (patch_source): корни, чей
/// params.sourceHandle совпадает с патчнутым handle, плюс всё их downstream.
/// Граф не меняется — меняются байты за хэндлом, поэтому все прогонные
/// результаты, выведенные из этого источника, устарели (FR-1.6).
pub fn compute_invalidated_for_source(
    graph: &hexforge_core::Graph,
    source_handle: &str,
) -> Vec<String> {
    let mut stale: std::collections::BTreeSet<String> = Default::default();
    for (id, node) in graph.nodes.iter() {
        let is_consumer_of_handle = node.inputs.is_empty()
            && node
                .params
                .get("sourceHandle")
                .and_then(|v| v.as_str())
                .map(|h| h == source_handle)
                .unwrap_or(false);
        if !is_consumer_of_handle {
            continue;
        }
        for down in graph.downstream_of(*id) {
            stale.insert(down.to_string());
        }
    }
    stale.into_iter().collect()
}

fn emit_progress(
    on_progress: ProgressSink<'_>,
    node_id: &NodeId,
    bytes_processed: usize,
) {
    on_progress(&ProgressEvent {
        node_id: node_id.to_string(),
        bytes_processed: bytes_processed as u64,
        bytes_total: None,
    });
}

/// Ленивый пересчёт снапшота (FR-4.1/4.2, Time-Travel): воспроизводит его
/// выход из корневого источника через lineage-цепочку операций. Байты
/// результата не хранятся в истории — только content-hash'и, поэтому реплей
/// обязателен всякий раз, когда выход вытеснен из кэша или запрошен прыжок.
///
/// Контроль целостности на каждом шаге:
/// - вход корневого узла цепочки берётся из SourceStore по
///   `params.sourceHandle` и сверяется с `input_content_hash` корневого
///   снапшота (источник изменён/освобождён → InvalidInput);
/// - выход каждого шага сверяется с `output_content_hash` соответствующего
///   снапшота — расхождение означало бы недетерминизм операции и нарушало бы
///   FR-4.2, поэтому это Internal-ошибка, а не молчаливая подмена результата.
pub fn replay_snapshot(
    state: &AppState,
    snapshot_id: hexforge_core::SnapshotId,
) -> HexForgeResult<Arc<Vec<u8>>> {
    let lineage: Vec<hexforge_core::Snapshot> = {
        let history = state.history.read();
        history
            .lineage(snapshot_id)
            .into_iter()
            .cloned()
            .collect()
    };
    let Some(first) = lineage.first() else {
        return Err(HexForgeError::invalid_input(format!(
            "unknown snapshot: {snapshot_id}"
        )));
    };

    // Корень lineage обязан быть source-root: его узел живёт в текущем графе
    // и ссылается на источник по params.sourceHandle.
    let root_bytes: Arc<Vec<u8>> = {
        let graph = state.graph.read();
        let node = graph.nodes.get(&first.node_id).ok_or_else(|| {
            HexForgeError::internal_for_node(
                first.node_id,
                format!(
                    "cannot replay snapshot {snapshot_id}: root node {} is gone from the current graph",
                    first.node_id
                ),
            )
        })?
        .clone();
        drop(graph);

        if !node.inputs.is_empty() {
            return Err(HexForgeError::invalid_input(
                "cannot replay snapshot: its root snapshot is not a source node",
            ));
        }
        let handle_str =
            node.params
                .get("sourceHandle")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    HexForgeError::invalid_parameter(
                        "sourceHandle",
                        "root node of the lineage requires params.sourceHandle",
                    )
                })?;
        let handle = Uuid::parse_str(handle_str)
            .map_err(|_| HexForgeError::invalid_input(format!("'{handle_str}' is not a valid source handle")))?;
        let sources = state.sources.read();
        let entry = sources.get(&handle).ok_or_else(|| {
            HexForgeError::invalid_input(format!(
                "cannot replay snapshot {snapshot_id}: source {handle_str} has been released"
            ))
        })?;
        Arc::new(entry.as_bytes().to_vec())
    };

    // Целостность корня: фактические байты источника обязаны совпасть
    // с зафиксированным input_content_hash.
    if blake3::hash(root_bytes.as_slice()) != first.input_content_hash {
        return Err(HexForgeError::invalid_input(format!(
            "cannot replay snapshot {snapshot_id}: source bytes no longer match the recorded input hash"
        )));
    }

    let mut current = root_bytes;
    for (index, step) in lineage.iter().enumerate() {
        let node = OperationNode {
            id: step.node_id,
            operation_id: step.operation_id.clone(),
            operation_version: step.operation_version.clone(),
            params: step.params.clone(),
            inputs: vec![],
        };
        let transform = lookup_transform(&node, state)?;

        // Вход шага — выход предыдущего; для первого шага это корневой
        // источник, уже сверённый выше по input_content_hash.
        if index > 0 {
            let input_hash = blake3::hash(current.as_slice());
            if input_hash != step.input_content_hash {
                return Err(HexForgeError::internal_for_node(
                    step.node_id,
                    format!(
                        "replay mismatch at {}: input hash diverged from the recorded snapshot",
                        step.operation_id
                    ),
                ));
            }
        }

        // Реплей исполняет полный буфер через apply: chunked-путь здесь не
        // нужен (вход целиком в памяти), кэш намеренно не затрагивается.
        let ctx: &dyn ExecutionContext = &NullExecutionContext;
        let view = transform
            .apply(Cow::Borrowed(current.as_slice()), &node.params, ctx)
            .map_err(HexForgeError::from)?;
        let output: Arc<Vec<u8>> = Arc::new(view.into_owned());

        if Some(blake3::hash(output.as_slice())) != step.output_content_hash {
            return Err(HexForgeError::internal_for_node(
                step.node_id,
                format!(
                    "replay mismatch at {}@{}: output hash differs from the recorded snapshot \
                     (deterministic operations must reproduce their output exactly, FR-4.2)",
                    step.operation_id, step.operation_version
                ),
            ));
        }

        current = output;
    }

    Ok(current)
}

/// Чистый конструктор снапшота истории (FR-4.2): фиксирует операцию@версию,
/// параметры и content-hash'и входа/выхода — этого достаточно для
/// воспроизведения результата без хранения самих байтов.
pub(crate) fn build_snapshot(
    node: &OperationNode,
    parent: Option<hexforge_core::SnapshotId>,
    input_hash: blake3::Hash,
    output_hash: blake3::Hash,
) -> hexforge_core::Snapshot {
    hexforge_core::Snapshot {
        id: Uuid::new_v4(),
        parent,
        node_id: node.id,
        operation_id: node.operation_id.clone(),
        operation_version: node.operation_version.clone(),
        params: node.params.clone(),
        input_content_hash: input_hash,
        output_content_hash: Some(output_hash),
    }
}

/// Записывает снапшот успешно выполненного узла в историю. Parent — текущая
/// голова истории на момент записи: MVP строит линейную цепочку, ветвление из
/// произвольной точки появится вместе с UI Time-Travel (структура History уже
/// DAG-ready). Локи берутся последовательно и не вложены в graph/sources —
/// порядок захвата всегда history → (graph|sources), дедлок невозможен.
/// Пишет снапшот результата стадии/узла с дедупликацией по ключу
/// воспроизводимости: повторное состояние НЕ создаёт новый узел DAG —
/// голова истории переезжает на существующий снапшот (FR-4.2).
/// Возвращает id актуального снапшота состояния.
pub(crate) fn record_output(
    node: &OperationNode,
    state: &AppState,
    input_hash: blake3::Hash,
    output_hash: blake3::Hash,
) -> hexforge_core::SnapshotId {
    let key = hexforge_core::reproducibility_key(
        &node.operation_id,
        &node.operation_version,
        &input_hash.to_hex()[..],
        &node.params,
    );
    // ВАЖНО: результат read-guard ограничен отдельной инструкцией —
    // if let со скрутини протянул бы guard в write-ветку (дедлок).
    let existing = { state.history.read().find_by_key(&key) };
    if let Some(existing) = existing {
        state.history.write().current = Some(existing);
        return existing;
    }
    let snapshot = build_snapshot(node, state.history.read().current, input_hash, output_hash);
    let id = snapshot.id;
    state.history.write().record(snapshot);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::error::HexForgeErrorKind;
    use crate::state::{AppState, SourceEntry};
    use base64::{engine::general_purpose, Engine as _};
    use hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES;
    use std::sync::atomic::AtomicBool;
    use hexforge_core::{ByteView, TransformError};

    fn no_progress(_event: &super::ProgressEvent) {}

    fn token() -> CancellationToken {
        Arc::new(AtomicBool::new(false))
    }

    fn root_node(state: &AppState, literal: &[u8], op: &str) -> NodeId {
        let handle = state.sources.write().insert(SourceEntry::InMemory(literal.to_vec()));
        let id = NodeId::new_v4();
        state.graph.write().insert_node(OperationNode {
            id,
            operation_id: op.into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": handle.to_string() }),
            inputs: vec![],
        });
        id
    }

    fn child(state: &AppState, op: &str, input: NodeId) -> NodeId {
        let id = NodeId::new_v4();
        state.graph.write().insert_node(OperationNode {
            id,
            operation_id: op.into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![input],
        });
        id
    }

    #[test]
    fn golden_chain_matches_previous_executor_semantics() {
        // Регрессионный якорь переноса resolve_node_output → scheduler:
        // rot13 → base64 над "Hello" обязан дать идентичный результат.
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"Hello", "text.rot13");
        let encode = child(&state, "encoding.base64.encode", root);

        let out = execute_chain(&state, &encode, &token(), &no_progress).unwrap();
        let expected = general_purpose::STANDARD.encode(rot13(b"Hello"));
        assert_eq!(out.as_slice(), expected.into_bytes().as_slice());
    }

    #[test]
    fn chunked_path_handles_multi_chunk_input() {
        // Вход больше DEFAULT_CHUNK_SIZE_BYTES форсирует несколько apply_chunk;
        // результат обязан совпасть с одноразовым apply (байто-независимые ops).
        let big: Vec<u8> = b"aBcDeF".iter().copied().cycle().take(DEFAULT_CHUNK_SIZE_BYTES + 777).collect();
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, &big, "text.rot13");

        let out = execute_chain(&state, &root, &token(), &no_progress).unwrap();
        assert_eq!(out.len(), big.len());
        for (i, (orig, conv)) in big.iter().zip(out.iter()).enumerate() {
            let expected = match orig {
                b'a'..=b'z' => b'a' + (orig - b'a' + 13) % 26,
                b'A'..=b'Z' => b'A' + (orig - b'A' + 13) % 26,
                other => *other,
            };
            assert_eq!(*conv, expected, "mismatch at byte {i}");
        }
    }

    #[test]
    fn cache_hit_skips_execution_and_records_snapshot() {
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"cached", "encoding.hex.encode");

        let first = execute_chain(&state, &root, &token(), &no_progress).unwrap();
        assert_eq!(first.as_slice(), b"636163686564");
        let misses_after_first = state.cache.lock().misses;

        let second = execute_chain(&state, &root, &token(), &no_progress).unwrap();
        assert_eq!(second.as_slice(), b"636163686564");
        {
            let cache = state.cache.lock();
            assert_eq!(cache.hits, 1, "second run must be a cache hit");
            assert_eq!(cache.misses, misses_after_first);
        }
        // История — граф СОСТОЯНИЙ: повтор идентичного запуска не создаёт
        // дубликат снапшота (head остаётся на месте).
        assert_eq!(state.history.read().order.len(), 1);
    }

    #[test]
    fn cache_distinguishes_params() {
        // Валидный base64-литерал: декодируется обоими алфавитами (не содержит
        // "+" и "/"), поэтому различие ключей кэша даёт именно params,
        // а не успех/ошибка выполнения.
        let state = AppState::new(hexforge_ops::build_registry());
        let handle = state
            .sources
            .write()
            .insert(SourceEntry::InMemory(b"SGVsbG8=".to_vec()));
        let a = NodeId::new_v4();
        let b = NodeId::new_v4();
        state.graph.write().insert_node(OperationNode {
            id: a,
            operation_id: "encoding.base64.decode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": handle.to_string(), "alphabet": "standard" }),
            inputs: vec![],
        });
        state.graph.write().insert_node(OperationNode {
            id: b,
            operation_id: "encoding.base64.decode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": handle.to_string(), "alphabet": "url_safe" }),
            inputs: vec![],
        });

        let out_a = execute_chain(&state, &a, &token(), &no_progress).unwrap();
        let out_b = execute_chain(&state, &b, &token(), &no_progress).unwrap();
        assert_eq!(out_a.as_slice(), b"Hello", "fixture must decode cleanly");
        assert_eq!(out_b.as_slice(), b"Hello");

        let cache = state.cache.lock();
        assert_eq!(cache.misses, 2, "разные params — разные ключи кэша");
    }

    #[test]
    fn cache_evicts_oldest_within_budget() {
        // Бюджет 48 байт, каждый выход (hex от 16 байт) = 32 байта:
        // вставка каждой следующей записи вытесняет предыдущую (32+32 > 48),
        // поэтому после трёх прогонов в кэше остаётся только r3.
        let state = AppState::with_cache_budget(hexforge_ops::build_registry(), 48);
        let r1 = root_node(&state, &[0xAB; 16], "encoding.hex.encode");
        let r2 = root_node(&state, &[0xCD; 16], "encoding.hex.encode");
        let r3 = root_node(&state, &[0xEF; 16], "encoding.hex.encode");

        execute_chain(&state, &r1, &token(), &no_progress).unwrap();
        execute_chain(&state, &r2, &token(), &no_progress).unwrap();
        execute_chain(&state, &r3, &token(), &no_progress).unwrap();
        {
            let cache = state.cache.lock();
            assert_eq!(cache.misses, 3);
            assert_eq!(cache.hits, 0);
        }

        // r3 ещё жив → hit; r1 вытеснен → miss с пересчётом.
        execute_chain(&state, &r3, &token(), &no_progress).unwrap();
        execute_chain(&state, &r1, &token(), &no_progress).unwrap();
        let cache = state.cache.lock();
        assert_eq!(cache.hits, 1, "only r3 must remain cached");
        assert_eq!(cache.misses, 4, "evicted r1 must be recomputed");
    }

    #[test]
    fn pre_cancelled_token_yields_cancelled_error_without_snapshots() {
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"x", "text.rot13");

        let cancelled = Arc::new(AtomicBool::new(true));
        let err = execute_chain(&state, &root, &cancelled, &no_progress).unwrap_err();

        assert_eq!(err.kind, HexForgeErrorKind::Cancelled);
        assert!(err.message.contains("was cancelled"));
        assert_eq!(err.node_id.as_deref(), Some(root.to_string().as_str()));
        assert!(state.history.read().order.is_empty());
    }

    #[test]
    fn cancel_between_nodes_stops_downstream() {
        // Чекпоинт в начале resolve_node: выставленный заранее токен роняет
        // запрошенный узел ошибкой Cancelled ещё до обращения к реестру.
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"hi", "text.rot13");
        let encode = child(&state, "encoding.base64.encode", root);

        execute_chain(&state, &root, &token(), &no_progress).unwrap(); // корень исполняем заранее

        let t = token();
        t.store(true, Ordering::Relaxed);
        let err = execute_chain(&state, &encode, &t, &no_progress).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::Cancelled);
    }

    #[test]
    fn merge_concat_executes_two_branches_in_order() {

        // Ветка A: hex.decode("4865") => "He"
        // Ветка B: rot13("y!")     => "l!"
        // concat([A, B])           => "Hel!"  (порядок inputs значим)
        // base64 сверху            => контрактный результат цепочки.
        let state = AppState::new(hexforge_ops::build_registry());
        let ha = state.sources.write().insert(SourceEntry::InMemory(b"4865".to_vec()));
        let hb = state.sources.write().insert(SourceEntry::InMemory(b"y!".to_vec()));

        let na = NodeId::new_v4();
        let nb = NodeId::new_v4();
        let nc = NodeId::new_v4();
        let nb64 = NodeId::new_v4();

        state.graph.write().insert_node(OperationNode {
            id: na,
            operation_id: "encoding.hex.decode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": ha.to_string() }),
            inputs: vec![],
        });
        state.graph.write().insert_node(OperationNode {
            id: nb,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": hb.to_string() }),
            inputs: vec![],
        });
        state.graph.write().insert_node(OperationNode {
            id: nc,
            operation_id: "streaming.concat".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![na, nb],
        });
        state.graph.write().insert_node(OperationNode {
            id: nb64,
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![nc],
        });

        let out = execute_chain(&state, &nb64, &token(), &no_progress).unwrap();
        let expected = general_purpose::STANDARD.encode(b"Hel!");
        assert_eq!(out.as_slice(), expected.into_bytes().as_slice());

        // История: по снапшоту на каждый выполненный узел DAG (4 узла).
        assert_eq!(state.history.read().order.len(), 4);
    }

    #[test]
    fn multi_input_without_merge_transform_rejected() {
        let state = AppState::new(hexforge_ops::build_registry());
        let h = state.sources.write().insert(SourceEntry::InMemory(b"a".to_vec()));
        let b1 = root_node(&state, b"one", "text.rot13");
        let b2 = root_node(&state, b"two", "text.rot13");
        let bad = NodeId::new_v4();
        state.graph.write().insert_node(OperationNode {
            id: bad,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": h.to_string() }),
            inputs: vec![b1, b2],
        });

        let err = execute_chain(&state, &bad, &token(), &no_progress).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        assert!(err.message.contains("does not support 2 inputs"));
    }

    #[test]
    fn replay_snapshot_reproduces_middle_of_chain() {
        // Цепочка rot13 → base64; прыжок к СРЕДНЕМУ снапшоту обязан выдать
        // ровно тот выход, который был зафиксирован при исполнении.
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"Hello", "text.rot13");
        let encode = child(&state, "encoding.base64.encode", root);

        let final_out = execute_chain(&state, &encode, &token(), &no_progress).unwrap();
        let (root_snap, encode_snap) = {
            let history = state.history.read();
            let snaps = history.ordered_snapshots();
            (snaps[0].id, snaps[1].id)
        };

        // Прыжок к корню цепочки: выход = вход base64-узла.
        let replayed_root = replay_snapshot(&state, root_snap).unwrap();
        assert_eq!(replayed_root.as_slice(), b"Uryyb");

        // Прыжок к последнему снапшоту: совпадает с обычным запуском.
        let replayed_final = replay_snapshot(&state, encode_snap).unwrap();
        assert_eq!(replayed_final.as_slice(), final_out.as_slice());
    }

    #[test]
    fn replay_fails_for_unknown_snapshot() {
        let state = AppState::new(hexforge_ops::build_registry());
        let err =
            replay_snapshot(&state, hexforge_core::SnapshotId::new_v4()).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        assert!(err.message.contains("unknown snapshot"));
    }

    #[test]
    fn replay_detects_mutated_source_bytes() {
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"original", "text.rot13");
        execute_chain(&state, &root, &token(), &no_progress).unwrap();
        let snap_id = state
            .history
            .read()
            .ordered_snapshots()
            .first()
            .map(|s| s.id)
            .expect("one snapshot after successful run");

        // Подменяем байты источника под тем же handle — content-hash разойдётся.
        {
            let mut sources = state.sources.write();
            let node = state.graph.read().nodes.get(&root).cloned().unwrap();
            let h = Uuid::parse_str(
                node.params.get("sourceHandle").and_then(|v| v.as_str()).unwrap(),
            )
            .unwrap();
            sources.replace(h, SourceEntry::InMemory(b"hacked!!".to_vec()));
        }

        let err = replay_snapshot(&state, snap_id).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::InvalidInput);
        assert!(err.message.contains("no longer match the recorded input hash"));
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
    fn build_snapshot_fixes_reproducibility_fields() {
        let node_id = NodeId::new_v4();
        let node = OperationNode {
            id: node_id,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![],
        };
        let parent = Uuid::new_v4();
        let input_hash = blake3::hash(b"input-bytes");
        let output_hash = blake3::hash(b"output-bytes");

        let snap = build_snapshot(&node, Some(parent), input_hash, output_hash);

        assert_ne!(snap.id, parent, "snapshot id must be freshly minted");
        assert_eq!(snap.parent, Some(parent));
        assert_eq!(snap.node_id, node_id);
        assert_eq!(snap.operation_id, "text.rot13");
        assert_eq!(snap.input_content_hash, input_hash);
        assert_eq!(snap.output_content_hash, Some(output_hash));

        let key = snap.reproducibility_key();
        assert!(key.starts_with("text.rot13@1.0.0::"));
        assert!(key.contains(&input_hash.to_hex()[..]));
    }

    /// Операция, опрашивающая ctx.is_cancelled() внутри apply и «взводящая»
    /// флаг при входе: тест-наблюдатель ждёт взведения и только тогда отменяет,
    /// гарантируя, что сигнал проверяется ИМЕННО внутри долгого apply.
    struct CancelAwareOp {
        armed: Arc<AtomicBool>,
    }

    impl hexforge_core::Transform for CancelAwareOp {
        fn id(&self) -> &'static str {
            "test.cancel-aware"
        }
        fn version(&self) -> &'static str {
            "1.0.0"
        }
        fn display_name(&self) -> &'static str {
            "CancelAware"
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
        fn apply<'a>(
            &self,
            _input: ByteView<'a>,
            _params: &serde_json::Value,
            ctx: &dyn ExecutionContext,
        ) -> Result<ByteView<'a>, TransformError> {
            self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
            // Активное ожидание сигнала с yield: детерминированно ловит проводку
            // токена независимо от параллельности тестов.
            let mut iters: u64 = 0;
            loop {
                if ctx.is_cancelled() {
                    return Err(TransformError::Internal("cancelled inside apply".into()));
                }
                iters += 1;
                if iters > 100_000_000 {
                    return Err(TransformError::Internal(
                        "op finished without observing cancellation".into(),
                    ));
                }
                if iters % 1024 == 0 {
                    std::thread::yield_now();
                }
                let _ = std::hint::black_box(iters);
            }
        }
    }

    #[test]
    fn cancellation_reaches_transform_context_mid_apply() {
        let mut state = AppState::new(hexforge_ops::build_registry());
        let armed = Arc::new(AtomicBool::new(false));
        let op: &'static CancelAwareOp =
            Box::leak(Box::new(CancelAwareOp { armed: Arc::clone(&armed) }));
        state.registry.register(op);
        let root = root_node(&state, &[0u8; 4096], "test.cancel-aware");

        let token = Arc::new(AtomicBool::new(false));

        // Наблюдатель: как только операция вошла в apply — отменяем.
        let watcher_token = Arc::clone(&token);
        let watcher_arm = Arc::clone(&armed);
        let handle = std::thread::spawn(move || {
            let mut spins = 0u64;
            while !watcher_arm.load(std::sync::atomic::Ordering::SeqCst) {
                spins += 1;
                if spins > 500_000_000 {
                    panic!("cancel-aware op never started");
                }
                std::hint::spin_loop();
                if spins % 4096 == 0 {
                    std::thread::yield_now();
                }
            }
            watcher_token.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let err = execute_chain(&state, &root, &token, &no_progress).unwrap_err();
        handle.join().expect("watcher must not panic");

        // Ошибка пришла ИЗ операции (Internal с её сообщением), а не от
        // чекпоинта планировщика — значит RunContext дотянулся внутрь apply.
        assert_eq!(err.kind, HexForgeErrorKind::Internal);
        assert!(err.message.contains("cancelled inside apply"));
        assert!(state.history.read().order.is_empty());
    }

    /// Стримовая версия cancel-aware операции: чекпоинт в каждом чанке.
    struct CancelAwareStreamOp {
        armed: Arc<AtomicBool>,
    }

    impl hexforge_core::Transform for CancelAwareStreamOp {
        fn id(&self) -> &'static str {
            "test.cancel-stream"
        }
        fn version(&self) -> &'static str {
            "1.0.0"
        }
        fn display_name(&self) -> &'static str {
            "CancelStream"
        }
        fn category(&self) -> &'static str {
            "Test"
        }
        fn capabilities(&self) -> hexforge_core::TransformCapabilities {
            hexforge_core::TransformCapabilities {
                deterministic: true,
                streamable: true,
                memory_cost: hexforge_core::MemoryCost::PerChunk,
            }
        }
        fn apply<'a>(
            &self,
            input: ByteView<'a>,
            _params: &serde_json::Value,
            ctx: &dyn ExecutionContext,
        ) -> Result<ByteView<'a>, TransformError> {
            self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
            if ctx.is_cancelled() {
                return Err(TransformError::Internal("cancelled inside apply".into()));
            }
            Ok(Cow::Owned(input.to_vec()))
        }
        fn apply_chunk(
            &self,
            chunk: &[u8],
            _is_last: bool,
            _state: &mut Box<dyn std::any::Any + Send>,
            _params: &serde_json::Value,
            ctx: &dyn ExecutionContext,
        ) -> Result<Vec<u8>, TransformError> {
            self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
            if ctx.is_cancelled() {
                return Err(TransformError::Internal("cancelled inside apply".into()));
            }
            Ok(chunk.to_vec())
        }
    }

    #[test]
    fn parallel_pipeline_propagates_mid_apply_cancellation() {
        // rot13 → cancel-stream: обе стадии streamable, вход > порога →
        // параллельный конвейер. Отмена выставляется наблюдателем ПОСЛЕ входа
        // в apply второй стадии; ошибка обязана прийти из операции через
        // канал, а не от чекпоинта планировщика.
        let big: Vec<u8> =
            vec![7u8; hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES + 999];

        let mut state = AppState::new(hexforge_ops::build_registry());
        let armed = Arc::new(AtomicBool::new(false));
        let op: &'static CancelAwareStreamOp =
            Box::leak(Box::new(CancelAwareStreamOp { armed: Arc::clone(&armed) }));
        state.registry.register(op);

        let root = root_node(&state, &big, "text.rot13");
        let cs = child(&state, "test.cancel-stream", root);

        let token = Arc::new(AtomicBool::new(false));
        let watcher_token = Arc::clone(&token);
        let watcher_arm = Arc::clone(&armed);
        let handle = std::thread::spawn(move || {
            let mut spins = 0u64;
            while !watcher_arm.load(std::sync::atomic::Ordering::SeqCst) {
                spins += 1;
                if spins > 200_000_000 {
                    panic!("stream op never started");
                }
                std::hint::spin_loop();
                if spins % 4096 == 0 {
                    std::thread::yield_now();
                }
            }
            watcher_token.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let err = execute_chain(&state, &cs, &token, &no_progress).unwrap_err();
        handle.join().expect("watcher must not panic");

        assert_eq!(err.kind, HexForgeErrorKind::Internal);
        assert!(err.message.contains("cancelled inside apply"));
        assert!(state.history.read().order.is_empty());
    }

    #[test]
    fn sequential_fusion_small_input_matches_reference() {
        // Вход меньше порога → диспетчер выбирает последовательный fusion;
        // результат обязан совпадать с прямым вычислением.

        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"small", "text.rot13");
        let enc = child(&state, "encoding.hex.encode", root);

        let out = execute_chain(&state, &enc, &token(), &no_progress).unwrap();
        let rotated: Vec<u8> = b"small"
            .iter()
            .map(|b| match b {
                b'a'..=b'z' => b'a' + (b - b'a' + 13) % 26,
                b'A'..=b'Z' => b'A' + (b - b'A' + 13) % 26,
                other => *other,
            })
            .collect();
        let expected_hex: Vec<u8> = rotated
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .into_bytes();
        assert_eq!(out.as_slice(), expected_hex.as_slice());
    }

    fn mk_node(id: NodeId, op: &str, params: serde_json::Value, inputs: Vec<NodeId>) -> OperationNode {
        OperationNode {
            id,
            operation_id: op.into(),
            operation_version: "1.0.0".into(),
            params,
            inputs,
        }
    }

    #[test]
    fn compute_invalidated_params_change_marks_downstream_only() {
        // root → mid → sink; меняем params ТОЛЬКО у root:
        // stale = {root, mid, sink} (всё downstream), ничего лишнего.
        let (a, b, c) = (NodeId::new_v4(), NodeId::new_v4(), NodeId::new_v4());
        let old = hexforge_core::Graph::from_nodes(vec![
            mk_node(a, "text.rot13", json!({}), vec![]),
            mk_node(b, "encoding.hex.encode", json!({}), vec![a]),
            mk_node(c, "encoding.base64.encode", json!({}), vec![b]),
        ]);
        let new = hexforge_core::Graph::from_nodes(vec![
            mk_node(a, "text.rot13", json!({"k": 1}), vec![]),
            mk_node(b, "encoding.hex.encode", json!({}), vec![a]),
            mk_node(c, "encoding.base64.encode", json!({}), vec![b]),
        ]);

        let stale = compute_invalidated(&old, &new);
        // Порядок события детерминирован (лексикографический по id-строке).
        let mut expect = vec![a.to_string(), b.to_string(), c.to_string()];
        expect.sort();
        assert_eq!(stale, expect);
    }

    #[test]
    fn compute_invalidated_untouched_branch_stays_fresh() {
        // fork: root → A и root → B; меняем ветку A — ветка B не затронута.
        let (r, a, b) = (NodeId::new_v4(), NodeId::new_v4(), NodeId::new_v4());
        let nodes_old = || {
            hexforge_core::Graph::from_nodes(vec![
                mk_node(r, "text.rot13", json!({}), vec![]),
                mk_node(a, "encoding.hex.encode", json!({}), vec![r]),
                mk_node(b, "encoding.hex.encode", json!({}), vec![r]),
            ])
        };
        let nodes_new = hexforge_core::Graph::from_nodes(vec![
            mk_node(r, "text.rot13", json!({}), vec![]),
            mk_node(a, "encoding.hex.encode", json!({"upper": true}), vec![r]),
            mk_node(b, "encoding.hex.encode", json!({}), vec![r]),
        ]);

        let stale = compute_invalidated(&nodes_old(), &nodes_new);
        assert_eq!(stale, vec![a.to_string()]);
    }

    #[test]
    fn compute_invalidates_new_and_op_changed_but_not_identical() {
        let (a, b) = (NodeId::new_v4(), NodeId::new_v4());
        let old = hexforge_core::Graph::from_nodes(vec![
            mk_node(a, "text.rot13", json!({}), vec![]),
        ]);
        // b — новый узел; a не изменился.
        let new = hexforge_core::Graph::from_nodes(vec![
            mk_node(a, "text.rot13", json!({}), vec![]),
            mk_node(b, "encoding.hex.encode", json!({}), vec![a]),
        ]);
        let stale = compute_invalidated(&old, &new);
        assert_eq!(stale, vec![b.to_string()]);
    }

    #[test]
    fn invalidated_for_source_covers_only_its_downstream() {
        // Два независимых источника h1 и h2; патч h1 затрагивает цепочку A,
        // но не цепочку B.
        use serde_json::json;

        let (a1, a2, b1, b2) = (
            NodeId::new_v4(),
            NodeId::new_v4(),
            NodeId::new_v4(),
            NodeId::new_v4(),
        );
        let g = hexforge_core::Graph::from_nodes(vec![
            mk_node(a1, "text.rot13", json!({ "sourceHandle": "h1" }), vec![]),
            mk_node(a2, "encoding.hex.encode", json!({}), vec![a1]),
            mk_node(b1, "text.rot13", json!({ "sourceHandle": "h2" }), vec![]),
            mk_node(b2, "encoding.hex.encode", json!({}), vec![b1]),
        ]);

        let stale = compute_invalidated_for_source(&g, "h1");
        assert_eq!(stale.len(), 2);
        assert!(stale.contains(&a1.to_string()));
        assert!(stale.contains(&a2.to_string()));
        assert!(!stale.contains(&b1.to_string()));

        // Патч неизвестного handle — пустой набор.
        assert!(compute_invalidated_for_source(&g, "nope").is_empty());
    }

    #[test]
    fn compute_invalidated_no_changes_empty() {
        let (a, b) = (NodeId::new_v4(), NodeId::new_v4());
        let g = hexforge_core::Graph::from_nodes(vec![
            mk_node(a, "text.rot13", json!({}), vec![]),
            mk_node(b, "encoding.hex.encode", json!({}), vec![a]),
        ]);
        assert!(compute_invalidated(&g, &g).is_empty());
    }

    #[test]
    fn jump_then_new_run_forks_history_dag() {
        // FR-4.1 ("аналог Git"): после прыжка к снапшоту в середине цепочки
        // новый запуск обязан породить ВЕТКУ — два снапшота с общим родителем,
        // а не линейное продолжение старого хвоста.
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"Hello", "text.rot13");
        let encode = child(&state, "encoding.base64.encode", root);
        execute_chain(&state, &encode, &token(), &no_progress).unwrap();

        // Линейная история: root ← encode (родительская ссылка).
        let (root_snap_id, _tail) = {
            let history = state.history.read();
            let snaps = history.ordered_snapshots();
            (snaps[0].id, snaps[1].id)
        };

        // Прыжок к корню: голова = корневой снапшот.
        replay_snapshot(&state, root_snap_id).unwrap();
        state.history.write().current = Some(root_snap_id);

        // Новый запуск ДРУГОЙ операции от этой головы → ветка.
        let other = child(&state, "encoding.hex.encode", root);
        execute_chain(&state, &other, &token(), &no_progress).unwrap();

        let history = state.history.read();
        assert_eq!(
            history.order.len(),
            3,
            "root + original tail + new branch node"
        );
        let branch = &history.snapshots[&history.order[2]];
        assert_eq!(branch.node_id, other);
        // Ключевое: родитель нового снапшота — точка прыжка, а не прежний хвост.
        assert_eq!(branch.parent, Some(root_snap_id));

        // Lineage ветки: root → branch (хвост исходной цепочки не в пути).
        let lin = history.lineage(branch.id);
        assert_eq!(lin.len(), 2);
        assert_eq!(lin[0].id, root_snap_id);
        assert_eq!(lin[1].id, branch.id);

        // Выход ветки корректен: hex(rot13("Hello")) — хэш от hex-строки.
        let expected_hex = hex_encode(b"Uryyb");
        assert_eq!(
            branch.output_content_hash,
            Some(blake3::hash(expected_hex.as_slice()))
        );
        drop(history);

        // Прыжок в конец новой ветки воспроизводит её результат.
        let out = replay_snapshot(&state, history_current(&state)).unwrap();
        assert_eq!(out.as_slice(), expected_hex.as_slice());
    }

    fn history_current(state: &AppState) -> hexforge_core::SnapshotId {
        state.history.read().current.expect("history has head")
    }

    fn hex_encode(data: &[u8]) -> Vec<u8> {
        data.iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .into_bytes()
    }
    #[test]
    fn fused_stream_run_multi_chunk_correct_and_cached_once() {
        // rot13 → hex.encode: ОБЕ стадии streamable → слияние в один чанк-цикл
        // над входом больше 1 МиБ (несколько чанков на стадию).

        let big: Vec<u8> = b"aBcD"
            .iter()
            .copied()
            .cycle()
            .take(hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES + 1234)
            .collect();
        let rotated: Vec<u8> = big
            .iter()
            .map(|b| match b {
                b'a'..=b'z' => b'a' + (b - b'a' + 13) % 26,
                b'A'..=b'Z' => b'A' + (b - b'A' + 13) % 26,
                other => *other,
            })
            .collect();
        let expected_hex: Vec<u8> = rotated
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            .into_bytes();

        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, &big, "text.rot13");
        let enc = child(&state, "encoding.hex.encode", root);

        let out = execute_chain(&state, &enc, &token(), &no_progress).unwrap();
        assert_eq!(out.as_slice(), expected_hex.as_slice());

        // История: по снапшоту на каждую стадию, вход ветки сшит с выходом
        // предыдущей (инкрементальные хэши корректны).
        {
            let history = state.history.read();
            assert_eq!(history.order.len(), 2);
            let snaps = history.ordered_snapshots();
            assert_eq!(snaps[0].output_content_hash, Some(snaps[1].input_content_hash));
        }

        // Кэш: только финальная стадия материализуется и пишется; повторный
        // запуск слитного прогона пересчитывает его (trade-off fusion —
        // ключ финальной стадии известен лишь после исполнения).
        execute_chain(&state, &enc, &token(), &no_progress).unwrap();
        let cache = state.cache.lock();
        assert_eq!(cache.hits, 0);
        assert_eq!(cache.misses, 0, "fusion исполняется без чтения кэша (trade-off)");
        // Но запись в кэше есть — ею воспользуется НЕ-слитный путь
        // (например merge/не-streamable потребитель через get()).
        assert_eq!(cache.entries_len(), 1);
    }

}
