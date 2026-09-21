//! Verified plugins as Transform operations in Graph/Recipe.
//!
//! Real template-built component (`plugins/example-wit/plugin.wasm`)
//! registered through the guarded `AppState::register_plugin` path, then
//! driven through the scheduler like any builtin: execute, snapshot,
//! replay, version strictness, and the missing-plugin install hint.

use hexforge_core::graph::OperationNode;
use hexforge_core::NodeId;
use hexforge_engine::scheduler;
use hexforge_engine::state::{AppState, SourceEntry};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");
const OP_ID: &str = "plugin:example.wit-uppercase";
const OP_VERSION: &str = "1.0.0";

fn token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn no_progress(_: &scheduler::ProgressEvent) {}

/// AppState with the REAL template component registered as `plugin:…`.
/// The staging wasm file is outside any grants store, so the instance
/// carries no privileged caps (template requests none).
fn plugin_state() -> AppState {
    let wasm_path = std::env::temp_dir().join(format!(
        "hexforge-recipe-node-{}.wasm",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&wasm_path, WASM).unwrap();
    let manifest: hexforge_plugin_host::PluginManifest = serde_json::from_str(MANIFEST).unwrap();
    let instance = hexforge_plugin_host::PluginInstance {
        manifest,
        wasm_path: wasm_path.to_string_lossy().into_owned(),
        pubkey_hex: String::new(),
        signature_hex: String::new(),
    };
    let runtime = Arc::new(hexforge_plugin_host::PluginRuntime::new(None).unwrap());
    let transform: Box<dyn hexforge_core::Transform> =
        Box::new(runtime.as_transform(instance).unwrap());
    let leaked: &'static dyn hexforge_core::Transform = Box::leak(transform);
    let state = AppState::new(hexforge_ops::build_registry());
    state
        .register_plugin(leaked)
        .expect("canonical id registers");
    assert_eq!(leaked.id(), OP_ID);
    state
}

fn plugin_node(id: NodeId, version: &str) -> OperationNode {
    OperationNode {
        id,
        operation_id: OP_ID.into(),
        operation_version: version.into(),
        params: serde_json::json!({}),
        inputs: vec![],
    }
}

#[test]
fn execute_verified_plugin_node_through_scheduler() {
    // Source echo is not a builtin; emulate input via base64 of known bytes
    // instead: chain text.uppercase builtin? No — plugin IS the transform:
    // feed it via a builtin producer node (base64 encode of "hello").
    let state = plugin_state();
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"hello".to_vec()));
    let n1 = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: n1,
        operation_id: "encoding.base64.encode".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let n2 = NodeId::new_v4();
    state.graph.write().insert_node(plugin_node(n2, OP_VERSION));
    // plugin node consumes n1's output
    {
        let mut g = state.graph.write();
        let node = g.nodes.get_mut(&n2).unwrap();
        node.inputs = vec![n1];
    }
    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    // base64("hello") = "aGVsbG8=" → uppercased by the plugin component.
    assert_eq!(out.as_slice(), b"AGVSBG8=");
}

#[test]
fn missing_plugin_node_reports_install_hint() {
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"x".to_vec()));
    let n1 = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: n1,
        operation_id: "plugin:ghost.op".into(),
        operation_version: "3.2.1".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let err = scheduler::execute_chain(&state, &n1, &token(), &no_progress).unwrap_err();
    let msg = &err.message;
    assert!(msg.contains("ghost.op"), "{msg}");
    assert!(msg.contains("3.2.1"), "{msg}");
    assert!(msg.contains("not installed"), "{msg}");
    assert!(!msg.contains("unknown operation"), "{msg}");
}

#[test]
fn builtin_missing_operation_message_unchanged() {
    let state = AppState::new(hexforge_ops::build_registry());
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"x".to_vec()));
    let n1 = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: n1,
        operation_id: "nosuch.op".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let err = scheduler::execute_chain(&state, &n1, &token(), &no_progress).unwrap_err();
    assert!(
        err.message.contains("unknown operation: nosuch.op"),
        "{:?}",
        err.message
    );
}

#[test]
fn wrong_plugin_version_never_silently_substituted() {
    let state = plugin_state();
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"x".to_vec()));
    let n1 = NodeId::new_v4();
    let mut node = plugin_node(n1, "9.9.9");
    node.params = serde_json::json!({ "sourceHandle": h.to_string() });
    state.graph.write().insert_node(node);
    let err = scheduler::execute_chain(&state, &n1, &token(), &no_progress).unwrap_err();
    assert!(
        err.message.contains("version mismatch"),
        "{:?}",
        err.message
    );
}

#[test]
fn replay_plugin_node_matches_and_pins_id_version() {
    let state = plugin_state();
    let h = state
        .sources
        .write()
        .insert(SourceEntry::InMemory(b"hello".to_vec()));
    let n1 = NodeId::new_v4();
    state.graph.write().insert_node(OperationNode {
        id: n1,
        operation_id: "encoding.base64.encode".into(),
        operation_version: "1.0.0".into(),
        params: serde_json::json!({ "sourceHandle": h.to_string() }),
        inputs: vec![],
    });
    let n2 = NodeId::new_v4();
    state.graph.write().insert_node(plugin_node(n2, OP_VERSION));
    state.graph.write().nodes.get_mut(&n2).unwrap().inputs = vec![n1];

    let out = scheduler::execute_chain(&state, &n2, &token(), &no_progress).unwrap();
    let history = state.history.read();
    let snaps = history.ordered_snapshots();
    let plugin_snap = snaps
        .iter()
        .find(|s| s.operation_id == OP_ID)
        .expect("plugin snapshot recorded");
    // Snapshot pins the canonical id + version + params (History v2, no drift).
    assert_eq!(plugin_snap.operation_version, OP_VERSION);
    assert_eq!(plugin_snap.params, serde_json::json!({}));
    let replayed = scheduler::replay_snapshot(&state, plugin_snap.id).unwrap();
    assert_eq!(replayed.as_slice(), out.as_slice());
}

#[test]
fn duplicate_plugin_registration_is_refused_state_intact() {
    let state = plugin_state();
    // Second registration of the same canonical id fails closed.
    let wasm_path = std::env::temp_dir().join(format!(
        "hexforge-recipe-node-dup-{}.wasm",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&wasm_path, WASM).unwrap();
    let manifest: hexforge_plugin_host::PluginManifest = serde_json::from_str(MANIFEST).unwrap();
    let instance = hexforge_plugin_host::PluginInstance {
        manifest,
        wasm_path: wasm_path.to_string_lossy().into_owned(),
        pubkey_hex: String::new(),
        signature_hex: String::new(),
    };
    let runtime = Arc::new(hexforge_plugin_host::PluginRuntime::new(None).unwrap());
    let dup: Box<dyn hexforge_core::Transform> = Box::new(runtime.as_transform(instance).unwrap());
    let dup: &'static dyn hexforge_core::Transform = Box::leak(dup);
    let err = state.register_plugin(dup).unwrap_err();
    assert!(err.contains("duplicate"), "{err}");
    // First registration still resolves.
    assert_eq!(state.registry.read().get(OP_ID).unwrap().id(), OP_ID);
}

#[test]
fn origin_defaults_to_builtin_for_existing_transforms() {
    let state = AppState::new(hexforge_ops::build_registry());
    let builtin = state.registry.read().get("encoding.base64.encode").unwrap();
    assert_eq!(builtin.origin(), "builtin");
    let plugin = plugin_state().registry.read().get(OP_ID).unwrap();
    assert_eq!(plugin.origin(), "plugin");
}
