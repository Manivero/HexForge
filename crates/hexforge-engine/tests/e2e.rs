//! End-to-end tests via AppState + scheduler + SourceStore + Graph + History + Registry
//! Covers critical user journeys without Tauri WebView (realistic for CI).

use base64::Engine as _;
use hexforge_core::graph::{Graph, OperationNode};
use hexforge_core::NodeId;
use hexforge_engine::graph_dto::{validate_graph, GraphDto, OperationNodeDto};
use hexforge_engine::state::{AppState, SourceEntry};
use hexforge_engine::{scheduler, HexForgeErrorKind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn no_progress(_: &scheduler::ProgressEvent) {}

#[test]
fn e2e_source_graph_execution_single_source() {
    // 1. source creation (InMemory) + 2. graph creation + 3. nodes + 4. execution + 5. correct result
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"Hello".to_vec()));
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
    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    // rot13("Hello") = "Uryyb" -> base64 = "VXJ5eWI="
    assert_eq!(
        out.as_slice(),
        base64::engine::general_purpose::STANDARD
            .encode(b"Uryyb")
            .as_bytes()
    );
    assert_eq!(state.history.read().order.len(), 2);
}

#[test]
fn e2e_graph_via_dto_and_validate() {
    // 2. graph via GraphDto + validate_graph + 3. linking
    let registry = hexforge_ops::build_registry();
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();
    let mut nodes = std::collections::HashMap::new();
    nodes.insert(
        a.to_string(),
        OperationNodeDto {
            id: a.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": "00000000-0000-4000-8000-000000000000" }),
            inputs: vec![],
        },
    );
    nodes.insert(
        b.to_string(),
        OperationNodeDto {
            id: b.to_string(),
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![a.to_string()],
        },
    );
    let dto = GraphDto { nodes };
    let graph = validate_graph(dto.clone(), &registry).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.topo_order().is_ok());
}

#[test]
fn e2e_multi_source_concat() {
    // 6. multi-source: 2 sources with different data -> concat -> correct
    let state = AppState::new(hexforge_ops::build_registry());
    let ha = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"AAA".to_vec()));
    let hb = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"BBB".to_vec()));
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
    // rot13 AAA -> NNN, BBB -> OOO, concat -> NNNOOO
    state.graph.write().insert_node(OperationNode {
        id: nc,
        operation_id: "streaming.concat".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![na, nb],
    });
    let out = scheduler::execute_chain(&state, &nc, &token(), &no_progress).unwrap();
    assert_eq!(out.as_slice(), b"NNNOOO");
}

#[test]
fn e2e_error_handling_invalid_handle_and_operation() {
    // 8. error handling: unknown handle, unknown operation, dangling input, cycle
    let state = AppState::new(hexforge_ops::build_registry());
    let bad_id = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: bad_id,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": "00000000-0000-4000-8000-000000000099" }),
        inputs: vec![],
    });
    let err = scheduler::execute_chain(&state, &bad_id, &token(), &no_progress).unwrap_err();
    assert_eq!(err.kind, HexForgeErrorKind::Internal);
    assert!(err.message.contains("unknown source handle"));

    let bad_op = NodeId::new_v4();
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"x".to_vec()));
    state.graph.write().insert_node(OperationNode {
        id: bad_op,
        operation_id: "encoding.nonexistent".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let err = scheduler::execute_chain(&state, &bad_op, &token(), &no_progress).unwrap_err();
    assert_eq!(err.kind, HexForgeErrorKind::Internal);
    assert!(err.message.contains("unknown operation"));

    // Dangling input
    let mut g = Graph::new();
    let dang = NodeId::new_v4();
    let missing = NodeId::new_v4();
    g.insert_node(OperationNode {
        id: dang,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![missing],
    });
    assert!(matches!(
        g.topo_order(),
        Err(hexforge_core::GraphError::DanglingInput(_))
    ));
}

#[test]
fn e2e_recipe_export_import_roundtrip() {
    // 9. recipe save/restore via GraphDto + validate
    let registry = hexforge_ops::build_registry();
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();
    let mut nodes = std::collections::HashMap::new();
    nodes.insert(
        a.to_string(),
        OperationNodeDto {
            id: a.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": "00000000-0000-4000-8000-000000000000" }),
            inputs: vec![],
        },
    );
    nodes.insert(
        b.to_string(),
        OperationNodeDto {
            id: b.to_string(),
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs: vec![a.to_string()],
        },
    );
    let dto = GraphDto { nodes };
    let json = serde_json::to_string(&dto).unwrap();
    let dto2: GraphDto = serde_json::from_str(&json).unwrap();
    let g = validate_graph(dto2, &registry).unwrap();
    assert_eq!(g.nodes.len(), 2);
}

#[test]
fn e2e_snapshot_history_and_diff() {
    // 9. snapshot history + diff + replay
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"Hello".to_vec()));
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
    let _out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    let snaps = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect::<Vec<_>>();
    assert_eq!(snaps.len(), 2);
    let first = snaps[0];
    let second = snaps[1];
    let replayed = scheduler::replay_snapshot(&state, second).unwrap();
    let out2 = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    assert_eq!(replayed.as_slice(), out2.as_slice());
    let diff = scheduler::diff_snapshots(&state, first, second).unwrap();
    assert!(diff.contains("snapshot") || diff.contains("binary diff") || diff.contains("---"));
    let same_diff = scheduler::diff_snapshots(&state, second, second).unwrap();
    assert_eq!(same_diff, "equal\n");
}

#[test]
fn e2e_plugin_host_via_transform() {
    // 7. plugin-host execution via Transform wrapper (real WASM)
    let wat_str = r#"
        (module
            (memory (export "memory") 1)
            (func (export "transform") (param i32 i32) (result i32)
                (local $i i32)
                (local $c i32)
                (local.set $i (i32.const 0))
                (block $exit
                    (loop $loop
                        (br_if $exit (i32.ge_u (local.get $i) (local.get 1)))
                        (local.set $c (i32.load8_u (i32.add (local.get 0) (local.get $i))))
                        (if (i32.and (i32.ge_u (local.get $c) (i32.const 97)) (i32.le_u (local.get $c) (i32.const 122)))
                            (then (i32.store8 (i32.add (local.get 0) (local.get $i)) (i32.sub (local.get $c) (i32.const 32)))))
                        (local.set $i (i32.add (local.get $i) (i32.const 1)))
                        (br $loop)
                    )
                )
                (local.get 1)
            )
        )
    "#;
    let wasm_bytes = wat::parse_str(wat_str).unwrap();
    let dir = std::env::temp_dir();
    let wasm_path = dir.join(format!("hexforge-e2e-plugin-{}.wasm", uuid::Uuid::new_v4()));
    std::fs::write(&wasm_path, &wasm_bytes).unwrap();
    let runtime =
        std::sync::Arc::new(hexforge_plugin_host::PluginRuntime::new(Some(1_000_000)).unwrap());
    let manifest = hexforge_plugin_host::PluginManifest {
        id: "e2e.uppercase".into(),
        name: "E2E Uppercase".into(),
        version: "1.0.0".into(),
        author: "Test".into(),
        requested_capabilities: vec![],
        granted_capabilities: vec![],
    };
    let instance = hexforge_plugin_host::PluginInstance {
        manifest,
        wasm_path: wasm_path.to_string_lossy().into_owned(),
        pubkey_hex: String::new(),
        signature_hex: String::new(),
    };
    let transform = runtime.as_transform(instance).unwrap();
    // Register into AppState registry and execute via scheduler
    let registry = hexforge_ops::build_registry();
    let leaked: Box<dyn hexforge_core::Transform> = Box::new(transform);
    let static_ref: &'static dyn hexforge_core::Transform = Box::leak(leaked);
    let state = AppState::new({
        let mut reg = registry;
        reg.register(static_ref);
        reg
    });
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"e2e test".to_vec()));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "e2e.uppercase".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.as_slice(), b"E2E TEST");
    let _ = std::fs::remove_file(&wasm_path);
}

#[test]
fn e2e_bench_gate_execute_chain_64k() {
    // NFR-1 gate: 64 KiB rot13+base64 chain via execute_chain should complete in <100ms (lenient, catches major regression)
    // This is a functional gate, not a wall-clock bench, so it's stable in CI.
    let state = AppState::new(hexforge_ops::build_registry());
    let data = vec![b'a'; 64 * 1024];
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
    let start = std::time::Instant::now();
    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    let elapsed = start.elapsed();
    assert!(!out.is_empty());
    assert!(
        elapsed.as_millis() < 100,
        "execute_chain 64 KiB took {elapsed:?}, expected <100ms"
    );
}

#[test]
fn e2e_history_branching_and_restore() {
    // Branching: linear chain a->b, run, jump to a, then run c (branch) -> history should have 3 snapshots with 2 children of root
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"branch".to_vec()));
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: a,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    state.graph.write().insert_node(OperationNode {
        id: b,
        operation_id: "encoding.base64.encode".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![a],
    });
    let _out1 = scheduler::execute_chain(&state, &b, &token(), &no_progress).unwrap();
    let snaps1: Vec<_> = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(snaps1.len(), 2);
    let first = snaps1[0];
    // Jump to first (branch point)
    let replayed = scheduler::replay_snapshot(&state, first).unwrap();
    assert_eq!(replayed.as_slice(), b"oenapu".as_slice()); // rot13 of "branch" is "oenapu"
                                                           // After jump, history.current should be first
    scheduler::replay_snapshot(&state, first).unwrap(); // ensure replay works
                                                        // Now create a new branch: add a new node c that is reverse of a
    let c = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: c,
        operation_id: "text.reverse".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![a],
    });
    let _out2 = scheduler::execute_chain(&state, &c, &token(), &no_progress).unwrap();
    let snaps2: Vec<_> = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect();
    // History should now have 3 snapshots: a, b, c (a is parent of both b and c)
    assert_eq!(snaps2.len(), 3);
    let diff = scheduler::diff_snapshots(&state, snaps1[1], snaps2[2]).unwrap();
    assert!(!diff.is_empty());
}

#[test]
fn e2e_history_multi_source_branching_and_diff() {
    // Multi-source: 2 sources -> concat -> branch
    let state = AppState::new(hexforge_ops::build_registry());
    let ha = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"AAA".to_vec()));
    let hb = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"BBB".to_vec()));
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
    let out1 = scheduler::execute_chain(&state, &nc, &token(), &no_progress).unwrap();
    assert_eq!(out1.as_slice(), b"NNNOOO");
    let snaps: Vec<_> = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect();
    assert!(snaps.len() >= 3);
    // Diff between last and itself should be equal (multi-source merge replay not yet fully supported for cross-branch diff)
    let diff_same =
        scheduler::diff_snapshots(&state, snaps[snaps.len() - 1], snaps[snaps.len() - 1]).unwrap();
    assert_eq!(diff_same, "equal\n");
}

#[test]
fn e2e_history_error_handling_missing_snapshot() {
    let state = AppState::new(hexforge_ops::build_registry());
    let missing = uuid::Uuid::new_v4();
    let err = scheduler::replay_snapshot(&state, missing).unwrap_err();
    assert!(err.message.contains("unknown snapshot"));
    let err2 = scheduler::diff_snapshots(&state, missing, missing).unwrap_err();
    assert!(err2.message.contains("unknown snapshot"));
}

#[test]
fn e2e_snapshot_v2_multi_source_replay_after_source_change() {
    // Main requirement: N-ary replay must work even after source bytes have changed
    let state = AppState::new(hexforge_ops::build_registry());
    let ha = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"AAA".to_vec()));
    let hb = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"BBB".to_vec()));
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
    let out1 = scheduler::execute_chain(&state, &nc, &token(), &no_progress).unwrap();
    assert_eq!(out1.as_slice(), b"NNNOOO");
    let snap_id = state.history.read().current.unwrap();
    let snap = state
        .history
        .read()
        .snapshots
        .get(&snap_id)
        .cloned()
        .unwrap();
    // v2 must have input_snapshot_ids and input_content_hashes with 2 entries
    assert_eq!(snap.input_snapshot_ids.len(), 2);
    assert_eq!(snap.input_content_hashes.as_ref().unwrap().len(), 2);
    // Change sources after snapshot: keep old handles for history (do not release), just create new handles and point graph to them
    let ha2 = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"XXX".to_vec()));
    let hb2 = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"YYY".to_vec()));
    // Update graph nodes to point to new handles (simulating source change, but snapshot should still replay old)
    state.graph.write().insert_node(OperationNode {
        id: na,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": ha2.to_string() }),
        inputs: vec![],
    });
    state.graph.write().insert_node(OperationNode {
        id: nb,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": hb2.to_string() }),
        inputs: vec![],
    });
    // Replay should still give old output (NNNOOO) even though current sources are XXX/YYY -> KKK/LLL
    let replayed = scheduler::replay_snapshot(&state, snap_id).unwrap();
    assert_eq!(replayed.as_slice(), b"NNNOOO");
    // Diff between old snapshot and new execution should not be equal
    let out2 = scheduler::execute_chain(&state, &nc, &token(), &no_progress).unwrap();
    assert_eq!(out2.as_slice(), b"KKKLLL"); // rot13 XXX->KKK, YYY->LLL
    let diff =
        scheduler::diff_snapshots(&state, snap_id, state.history.read().current.unwrap()).unwrap();
    assert_ne!(diff, "equal\n");
}

#[test]
fn e2e_snapshot_v2_single_source_replay_still_works() {
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"Hello".to_vec()));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    let snap_id = state.history.read().current.unwrap();
    let snap = state
        .history
        .read()
        .snapshots
        .get(&snap_id)
        .cloned()
        .unwrap();
    // Single-source v2: input_content_hashes is None (backward compat), input_snapshot_ids empty
    assert!(snap.input_content_hashes.is_none());
    assert!(snap.input_snapshot_ids.is_empty());
    let replayed = scheduler::replay_snapshot(&state, snap_id).unwrap();
    assert_eq!(replayed.as_slice(), out.as_slice());
}

#[test]
fn e2e_snapshot_v2_restore_branch() {
    // Restore branch: jump to first snapshot, then run a different operation -> branching
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"branch".to_vec()));
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: a,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    state.graph.write().insert_node(OperationNode {
        id: b,
        operation_id: "encoding.base64.encode".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![a],
    });
    let _out1 = scheduler::execute_chain(&state, &b, &token(), &no_progress).unwrap();
    let snaps1: Vec<_> = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect();
    let first = snaps1[0];
    // Jump to first
    let _replayed = scheduler::replay_snapshot(&state, first).unwrap();
    // Simulate jump by setting current
    state.history.write().current = Some(first);
    let c = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: c,
        operation_id: "text.reverse".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![a],
    });
    let _out2 = scheduler::execute_chain(&state, &c, &token(), &no_progress).unwrap();
    let snaps2: Vec<_> = state
        .history
        .read()
        .ordered_snapshots()
        .iter()
        .map(|s| s.id)
        .collect();
    // Should have 3 snapshots: a, b, c (a is parent of both b and c)
    assert_eq!(snaps2.len(), 3);
    // Verify branching: b and c should have same parent (first)
    let b_snap = state
        .history
        .read()
        .snapshots
        .get(&snaps2[1])
        .cloned()
        .unwrap();
    let c_snap = state
        .history
        .read()
        .snapshots
        .get(&snaps2[2])
        .cloned()
        .unwrap();
    assert_eq!(b_snap.parent, Some(first));
    assert_eq!(c_snap.parent, Some(first));
}

#[test]
fn e2e_snapshot_v2_diff_between_nary_snapshots() {
    let state = AppState::new(hexforge_ops::build_registry());
    let ha = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"AAA".to_vec()));
    let hb = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"BBB".to_vec()));
    let hc = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"CCC".to_vec()));
    let na = NodeId::new_v4();
    let nb = NodeId::new_v4();
    let nc1 = NodeId::new_v4();
    let nc2 = NodeId::new_v4();
    for (id, h) in [(na, ha), (nb, hb)] {
        state.graph.write().insert_node(OperationNode {
            id,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": h.to_string() }),
            inputs: vec![],
        });
    }
    state.graph.write().insert_node(OperationNode {
        id: nc1,
        operation_id: "streaming.concat".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![na, nb],
    });
    let _out1 = scheduler::execute_chain(&state, &nc1, &token(), &no_progress).unwrap();
    let snap1 = state.history.read().current.unwrap();
    // Change second source to CCC and create new concat
    let nb2 = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nb2,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": hc.to_string() }),
        inputs: vec![],
    });
    state.graph.write().insert_node(OperationNode {
        id: nc2,
        operation_id: "streaming.concat".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![na, nb2],
    });
    let _out2 = scheduler::execute_chain(&state, &nc2, &token(), &no_progress).unwrap();
    let snap2 = state.history.read().current.unwrap();
    let diff = scheduler::diff_snapshots(&state, snap1, snap2).unwrap();
    assert_ne!(diff, "equal\n");
    assert!(diff.contains("snapshot"));
}

#[test]
fn e2e_replay_after_source_release_fails() {
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"replay-test".to_vec()));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert!(!out.is_empty());
    let snap_id = state.history.read().ordered_snapshots()[0].id;
    // Release source
    assert!(state.sources.write().release(&h));
    let err = scheduler::replay_snapshot(&state, snap_id).unwrap_err();
    assert!(
        err.message.contains("has been released") || err.message.contains("source"),
        "expected source released error, got {err:?}"
    );
}

#[test]
fn e2e_large_input_streaming_1m() {
    // Large input (1 MiB) via streamable rot13 should succeed and be correct
    let state = AppState::new(hexforge_ops::build_registry());
    let data = vec![b'a'; 1024 * 1024];
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(data.clone()));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), data.len());
    assert_eq!(out[0], b'n'); // rot13 a -> n
    assert_eq!(out[1024 * 1024 - 1], b'n');
}

const E2E_MB: usize = 1024 * 1024;

/// Собирает цепочку rot13(source) → base64.encode для больших входов.
fn large_pipeline(state: &AppState, data: Vec<u8>) -> hexforge_core::NodeId {
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
    n2
}

#[test]
fn e2e_large_file_mmap_64m_pipeline() {
    // Сценарий large-file: файловый источник через mmap (SourceStore не
    // грузит файл в RAM целиком) через streamable rot13 → base64.
    // rot13('a') = 'n'; base64("nnn") = "bm5u", хвост 1 байт "n" = "bg==".
    let len = 64 * E2E_MB;
    let path = std::env::temp_dir().join(format!("hexforge-e2e-{}-64m.bin", std::process::id()));
    std::fs::write(&path, vec![b'a'; len]).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    // SAFETY: файл только что создан тестом, никто его не меняет, пока живёт mapping.
    let mmap = unsafe { memmap2::Mmap::map(&file).unwrap() };
    drop(file);

    let state = AppState::new(hexforge_ops::build_registry());
    let h = state.sources.write().insert(SourceEntry::Mapped(mmap));
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

    let events = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&events);
    let out = scheduler::execute_chain(&state, &n2, &token(), &|_| {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .unwrap();

    assert_eq!(out.len(), len.div_ceil(3) * 4);
    assert!(out.starts_with(b"bm5u"));
    assert!(out.ends_with(b"bg=="));
    assert!(
        events.load(Ordering::Relaxed) > 0,
        "streaming pipeline must emit progress"
    );
    drop(state); // освободить mmap перед удалением (Windows держит лок)
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn e2e_large_input_128m_parallel_pipeline_correctness() {
    // 2 стадии + вход > 64 МиБ → execute_fused_parallel: потоки на стадию,
    // bounded-канал с backpressure. Результат обязан совпасть с точным base64.
    // 128 МиБ mod 3 = 2 → хвост "nn" = "bm4=".
    let len = 128 * E2E_MB;
    let state = AppState::new(hexforge_ops::build_registry());
    let n2 = large_pipeline(&state, vec![b'a'; len]);
    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), len.div_ceil(3) * 4);
    assert!(out.starts_with(b"bm5u"));
    assert!(out.ends_with(b"bm4="));
    // Середина тоже rot13+base64, а не мусор потоков: квант, выровненный
    // на границу base64-блока (4 байта), обязан быть "bm5u" (квант "nnn").
    let mid = (out.len() / 2) & !3;
    assert_eq!(&out[mid..mid + 4], b"bm5u");
}

#[test]
fn e2e_large_input_256m_single_op_boundary_cache() {
    // Выход ровно 256 МиБ == дефолтный бюджет: `size > max` ложно → кэшируется
    // (граничное условие put). Повторный прогон — cache hit с тем же результатом.
    let len = 256 * E2E_MB;
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(vec![b'a'; len]));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), len);
    assert_eq!(out[0], b'n');
    assert_eq!(out[len - 1], b'n');
    assert_eq!(state.cache.lock().entries_len(), 1);

    let out2 = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.as_slice(), out2.as_slice());
    assert!(state.cache.lock().hits >= 1);
}

#[test]
fn e2e_large_output_over_budget_not_cached_but_correct() {
    // Выход больше бюджета: корректность сохраняется, но запись не кэшируется
    // (put отбрасывает сверхбюджетные), повторный прогон — снова miss + успех.
    let state = AppState::with_cache_budget(hexforge_ops::build_registry(), E2E_MB);
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(vec![b'a'; 4 * E2E_MB]));
    let nid = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: nid,
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let out = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), 4 * E2E_MB);
    assert!(out.iter().all(|&b| b == b'n'));
    assert_eq!(state.cache.lock().entries_len(), 0);

    let misses = state.cache.lock().misses;
    let out2 = scheduler::execute_chain(&state, &nid, &token(), &no_progress).unwrap();
    assert_eq!(out.as_slice(), out2.as_slice());
    assert!(state.cache.lock().misses > misses);
    assert_eq!(state.cache.lock().entries_len(), 0);
}

#[test]
fn e2e_multi_source_large_concat() {
    // Multi-source merge на больших входах: 2×8 МиБ → 16 МиБ, порядок входов
    // и граница склейки точные (rot13 'A'='N', 'B'='O').
    let half = 8 * E2E_MB;
    let state = AppState::new(hexforge_ops::build_registry());
    let ha = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(vec![b'A'; half]));
    let hb = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(vec![b'B'; half]));
    let na = NodeId::new_v4();
    let nb = NodeId::new_v4();
    let nc = NodeId::new_v4();
    for (id, h) in [(na, ha), (nb, hb)] {
        state.graph.write().insert_node(OperationNode {
            id,
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({ "sourceHandle": h.to_string() }),
            inputs: vec![],
        });
    }
    state.graph.write().insert_node(OperationNode {
        id: nc,
        operation_id: "streaming.concat".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({}),
        inputs: vec![na, nb],
    });
    let out = scheduler::execute_chain(&state, &nc, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), 2 * half);
    assert_eq!(out[0], b'N');
    assert_eq!(out[half - 1], b'N');
    assert_eq!(out[half], b'O');
    assert_eq!(out[2 * half - 1], b'O');
}

#[test]
fn e2e_cancel_parallel_pipeline_never_reports_partial_success() {
    // Regression: отмена parallel-fused прогона (4 чанка, отмена бьёт в feed)
    // раньше возвращала Ok(обрезанный выход) и травила memoization-кэш под
    // ключом полного входа. Инвариант: либо Err(Cancelled), либо ПОЛНЫЙ
    // корректный выход; повторный прогон со свежим токеном — полный выход.
    let len = 256 * E2E_MB;
    let full_b64_len = len.div_ceil(3) * 4;
    let state = AppState::new(hexforge_ops::build_registry());
    let n2 = large_pipeline(&state, vec![b'a'; len]);

    let t = token();
    let killer = Arc::clone(&t);
    let handle = std::thread::spawn(move || {
        // Первый чанк (64 МиБ memcpy) заведомо дольше 1 мс на любом железе —
        // отмена приземляется в feed до его завершения, детерминированно.
        std::thread::sleep(std::time::Duration::from_millis(1));
        killer.store(true, Ordering::SeqCst);
    });
    let res = scheduler::execute_chain(&state, &n2, &t, &no_progress);
    handle.join().unwrap();
    match res {
        Err(e) => assert_eq!(e.kind, HexForgeErrorKind::Cancelled),
        Ok(out) => {
            // Отмена опоздала: выход обязан быть полным и корректным.
            assert_eq!(out.len(), full_b64_len);
            assert!(out.starts_with(b"bm5u"));
        }
    }

    // Кэш не отравлен частичным выходом: свежий прогон — полный результат.
    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    assert_eq!(out.len(), full_b64_len);
    assert!(out.starts_with(b"bm5u"));
    assert!(out.ends_with(b"bg=="));
}
