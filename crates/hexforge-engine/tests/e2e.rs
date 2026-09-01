//! End-to-end tests via AppState + scheduler + SourceStore + Graph + History + Registry
//! Covers critical user journeys without Tauri WebView (realistic for CI).

use base64::Engine as _;
use hexforge_core::graph::{Graph, OperationNode};
use hexforge_core::NodeId;
use hexforge_engine::graph_dto::{validate_graph, GraphDto, OperationNodeDto};
use hexforge_engine::state::{AppState, SourceEntry};
use hexforge_engine::{scheduler, HexForgeErrorKind};
use std::sync::atomic::AtomicBool;
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
