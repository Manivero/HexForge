//! Планировщик исполнения цепочки узлов (MVP-ядро `hexforge-stream`,
//! оркестрация живёт в src-tauri, т.к. обязана знать домен — см. docs/04 §6).
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
use hexforge_stream::chunk_ranges;
use serde::Serialize;
use std::borrow::Cow;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::Emitter;
use uuid::Uuid;

/// Событие прогресса выполнения (`op://progress`, 05-IPC-CONTRACT.md §events).
/// Поле `bytesTotal: null` соответствует TS `number | null`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub node_id: String,
    pub bytes_processed: u64,
    pub bytes_total: Option<u64>,
}

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
    emitter: Option<&tauri::AppHandle>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    resolve_node(root, state, token, emitter)
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
    emitter: Option<&tauri::AppHandle>,
) -> HexForgeResult<Arc<Vec<u8>>> {
    check_cancelled(node_id, token)?;

    let node = {
        let graph = state.graph.read();
        graph.nodes.get(node_id).cloned().ok_or_else(|| {
            HexForgeError::internal_for_node(node_id, format!("node {node_id} not found in current graph"))
        })?
    };

    // Входы: 0 — источник из SourceStore, 1 — рекурсивный выход родителя,
    // N — merge-ветка (порядок inputs — часть контракта операции).
    let inputs: Vec<Arc<Vec<u8>>> = match node.inputs.len() {
        0 => vec![resolve_source_input(&node, node_id, state)?],
        1 => {
            let parent_output = resolve_node(&node.inputs[0], state, token, emitter)?;
            check_cancelled(node_id, token)?;
            vec![parent_output]
        }
        _ => {
            let mut resolved = Vec::with_capacity(node.inputs.len());
            for input_id in &node.inputs {
                resolved.push(resolve_node(input_id, state, token, emitter)?);
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
        emit_progress(emitter, node_id, cached.len());
        record_snapshot(&node, state, primary_input_hash, blake3::hash(&cached));
        return Ok(cached);
    }

    let output: Arc<Vec<u8>> = if node.inputs.len() > 1 {
        execute_merge_node(&node, state, token, inputs)?
    } else {
        execute_unary_node(&node, state, token, Arc::clone(&inputs[0]))?
    };

    state.cache.lock().put(cache_key, Arc::clone(&output));
    emit_progress(emitter, node_id, output.len());
    record_snapshot(&node, state, primary_input_hash, blake3::hash(&output));

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
        let ctx: &dyn ExecutionContext = &NullExecutionContext;
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
    let mut chunk_state: Box<dyn std::any::Any> = Box::new(());
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

fn emit_progress(emitter: Option<&tauri::AppHandle>, node_id: &NodeId, bytes_processed: usize) {
    // Ошибка доставки сознательно игнорируется: отсутствие слушателя/закрытое
    // окно не должно валить выполнение узла.
    if let Some(app) = emitter {
        let _ = app.emit(
            "op://progress",
            ProgressEvent {
                node_id: node_id.to_string(),
                bytes_processed: bytes_processed as u64,
                bytes_total: None,
            },
        );
    }
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
pub(crate) fn record_snapshot(
    node: &OperationNode,
    state: &AppState,
    input_hash: blake3::Hash,
    output_hash: blake3::Hash,
) {
    let parent = state.history.read().current;
    let snapshot = build_snapshot(node, parent, input_hash, output_hash);
    state.history.write().record(snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HexForgeErrorKind;
    use crate::state::{AppState, SourceEntry};
    use base64::{engine::general_purpose, Engine as _};
    use hexforge_stream::DEFAULT_CHUNK_SIZE_BYTES;
    use std::sync::atomic::AtomicBool;

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

        let out = execute_chain(&state, &encode, &token(), None).unwrap();
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

        let out = execute_chain(&state, &root, &token(), None).unwrap();
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

        let first = execute_chain(&state, &root, &token(), None).unwrap();
        assert_eq!(first.as_slice(), b"636163686564");
        let misses_after_first = state.cache.lock().misses;

        let second = execute_chain(&state, &root, &token(), None).unwrap();
        assert_eq!(second.as_slice(), b"636163686564");
        {
            let cache = state.cache.lock();
            assert_eq!(cache.hits, 1, "second run must be a cache hit");
            assert_eq!(cache.misses, misses_after_first);
        }
        // История фиксирует оба запуска — кэш прозрачен для Time-Travel.
        assert_eq!(state.history.read().order.len(), 2);
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

        let out_a = execute_chain(&state, &a, &token(), None).unwrap();
        let out_b = execute_chain(&state, &b, &token(), None).unwrap();
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

        execute_chain(&state, &r1, &token(), None).unwrap();
        execute_chain(&state, &r2, &token(), None).unwrap();
        execute_chain(&state, &r3, &token(), None).unwrap();
        {
            let cache = state.cache.lock();
            assert_eq!(cache.misses, 3);
            assert_eq!(cache.hits, 0);
        }

        // r3 ещё жив → hit; r1 вытеснен → miss с пересчётом.
        execute_chain(&state, &r3, &token(), None).unwrap();
        execute_chain(&state, &r1, &token(), None).unwrap();
        let cache = state.cache.lock();
        assert_eq!(cache.hits, 1, "only r3 must remain cached");
        assert_eq!(cache.misses, 4, "evicted r1 must be recomputed");
    }

    #[test]
    fn pre_cancelled_token_yields_cancelled_error_without_snapshots() {
        let state = AppState::new(hexforge_ops::build_registry());
        let root = root_node(&state, b"x", "text.rot13");

        let cancelled = Arc::new(AtomicBool::new(true));
        let err = execute_chain(&state, &root, &cancelled, None).unwrap_err();

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

        execute_chain(&state, &root, &token(), None).unwrap(); // корень исполняем заранее

        let t = token();
        t.store(true, Ordering::Relaxed);
        let err = execute_chain(&state, &encode, &t, None).unwrap_err();
        assert_eq!(err.kind, HexForgeErrorKind::Cancelled);
    }

    #[test]
    fn merge_concat_executes_two_branches_in_order() {
        use base64::Engine as _;

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

        let out = execute_chain(&state, &nb64, &token(), None).unwrap();
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

        let err = execute_chain(&state, &bad, &token(), None).unwrap_err();
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

        let final_out = execute_chain(&state, &encode, &token(), None).unwrap();
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
        execute_chain(&state, &root, &token(), None).unwrap();
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
}
