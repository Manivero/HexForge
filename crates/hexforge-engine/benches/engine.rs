use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use hexforge_core::{graph::OperationNode, NodeId};
use hexforge_engine::state::{AppState, SourceEntry};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn bench_execute_chain_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("execute_chain_single");
    for size in [1024, 64 * 1024, 1024 * 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("rot13_base64_{size}B"), &size, |b, &size| {
            b.iter(|| {
                let state = AppState::new(hexforge_ops::build_registry());
                let data = vec![b'a'; size];
                let h = state.sources.write().insert(SourceEntry::InMemory(data));
                let n1 = NodeId::new_v4();
                let n2 = NodeId::new_v4();
                state.graph.write().insert_node(OperationNode {
                    id: n1,
                    operation_id: "text.rot13".into(),
                    operation_version: "1.0.0".into(),
                    params: serde_json::json!({ "sourceHandle": h.to_string() }),
                    inputs: vec![],
                });
                state.graph.write().insert_node(OperationNode {
                    id: n2,
                    operation_id: "encoding.base64.encode".into(),
                    operation_version: "1.0.0".into(),
                    params: serde_json::json!({}),
                    inputs: vec![n1],
                });
                let token = Arc::new(AtomicBool::new(false));
                hexforge_engine::scheduler::execute_chain(
                    black_box(&state),
                    black_box(&n2),
                    &token,
                    &|_| {},
                )
                .unwrap()
            })
        });
    }
    group.finish();
}

fn bench_execute_chain_multi_source(c: &mut Criterion) {
    let mut group = c.benchmark_group("execute_chain_multi_source");
    let size = 64 * 1024;
    group.throughput(Throughput::Bytes((size * 2) as u64));
    group.bench_function("concat_two_64KiB", |b| {
        b.iter(|| {
            let state = AppState::new(hexforge_ops::build_registry());
            let ha = state
                .sources
                .write()
                .insert(SourceEntry::InMemory(vec![b'a'; size]));
            let hb = state
                .sources
                .write()
                .insert(SourceEntry::InMemory(vec![b'b'; size]));
            let na = NodeId::new_v4();
            let nb = NodeId::new_v4();
            let nc = NodeId::new_v4();
            state.graph.write().insert_node(OperationNode {
                id: na,
                operation_id: "text.rot13".into(),
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
            let token = Arc::new(AtomicBool::new(false));
            hexforge_engine::scheduler::execute_chain(
                black_box(&state),
                black_box(&nc),
                &token,
                &|_| {},
            )
            .unwrap()
        })
    });
    group.finish();
}

fn bench_64m_pipeline(c: &mut Criterion) {
    // NFR-1 gate: 64 MiB default chunk handling should be <100ms for single streamable op
    // This bench uses 64 MiB of 'a' and a streamable rot13 -> base64 chain, similar to scheduler fusion
    c.benchmark_group("pipeline_64M")
        .throughput(Throughput::Bytes(64 * 1024 * 1024))
        .bench_function("rot13_64M", |b| {
            b.iter(|| {
                let state = AppState::new(hexforge_ops::build_registry());
                let data = vec![b'a'; 64 * 1024 * 1024];
                let h = state.sources.write().insert(SourceEntry::InMemory(data));
                let n1 = NodeId::new_v4();
                state.graph.write().insert_node(OperationNode {
                    id: n1,
                    operation_id: "text.rot13".into(),
                    operation_version: "1.0.0".into(),
                    params: serde_json::json!({ "sourceHandle": h.to_string() }),
                    inputs: vec![],
                });
                let token = Arc::new(AtomicBool::new(false));
                hexforge_engine::scheduler::execute_chain(
                    black_box(&state),
                    black_box(&n1),
                    &token,
                    &|_| {},
                )
                .unwrap()
            })
        });
}

criterion_group!(
    benches,
    bench_execute_chain_single,
    bench_execute_chain_multi_source,
    bench_64m_pipeline
);
criterion_main!(benches);
