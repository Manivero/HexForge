//! Интеграционные тесты run_recipe поверх временных файлов: happy-path
//! (rot13 → base64), битый JSON, неизвестная операция, два стока.

use base64::{engine::general_purpose, Engine as _};
use hexforge_core::graph::NodeId;
use serde_json::json;
use std::path::{Path, PathBuf};

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hf-cli-{}", uuid::Uuid::new_v4()));
    fs_extra_mkdir(&dir);
    dir
}

fn fs_extra_mkdir(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create temp dir");
}

fn write(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, contents).expect("write fixture");
    p
}

fn recipe_json(nodes: serde_json::Value) -> String {
    json!({ "nodes": nodes }).to_string()
}

#[test]
fn run_recipe_rot13_then_base64_end_to_end() {
    use hexforge_engine::graph_dto::OperationNodeDto;

    let dir = temp_dir();
    let root_id = NodeId::new_v4();
    let enc_id = NodeId::new_v4();

    // sourceHandle будет перезаписан CLI на --in; кладём заведомо невалидный
    // плейсхолдер — тест подтверждает, что биндинг действительно происходит.
    let nodes = json!({
        root_id.to_string(): OperationNodeDto {
            id: root_id.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: json!({ "sourceHandle": "00000000-0000-4000-8000-000000000000" }),
            inputs: vec![],
        },
        enc_id.to_string(): OperationNodeDto {
            id: enc_id.to_string(),
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![root_id.to_string()],
        }
    });

    let recipe = write(&dir, "recipe.hexforge", recipe_json(nodes).as_bytes());
    let input = write(&dir, "in.bin", b"Hello");
    let out = dir.join("out.bin");

    let summary = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        input.to_str().unwrap(),
        out.to_str().unwrap(),
    )
    .expect("recipe must run");

    // rot13("Hello") = "Uryyb" (5 байт) → base64 = 8 символов.
    assert_eq!(summary.output_bytes, 8);
    let written = std::fs::read(&out).unwrap();
    let expected = general_purpose::STANDARD.encode(b"Uryyb");
    assert_eq!(written, expected.into_bytes());
    assert_eq!(summary.executed_nodes, 2);
}

#[test]
fn run_recipe_rejects_invalid_json() {
    let dir = temp_dir();
    let recipe = write(&dir, "bad.json", b"{ nope");
    let err = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        recipe.to_str().unwrap(),
        dir.join("o.bin").to_str().unwrap(),
    )
    .unwrap_err();
    assert!(err.contains("not a valid recipe file"));
}

#[test]
fn run_recipe_rejects_missing_operation() {
    use hexforge_engine::graph_dto::OperationNodeDto;
    let dir = temp_dir();
    let id = NodeId::new_v4();
    let nodes = json!({
        id.to_string(): OperationNodeDto {
            id: id.to_string(),
            operation_id: "encoding.nonexistent".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![],
        }
    });
    let recipe = write(&dir, "r.json", recipe_json(nodes).as_bytes());
    let err = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        recipe.to_str().unwrap(),
        dir.join("o.bin").to_str().unwrap(),
    )
    .unwrap_err();
    assert!(err.contains("missing from registry"));
}

#[test]
fn run_recipe_requires_single_output_node() {
    use hexforge_engine::graph_dto::OperationNodeDto;
    let dir = temp_dir();
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();

    let mk = |id: &NodeId| OperationNodeDto {
        id: id.to_string(),
        operation_id: "text.rot13".into(),
        operation_version: "1.0.0".into(),
        params: json!({}),
        inputs: vec![],
    };
    let nodes = json!({
        a.to_string(): mk(&a),
        b.to_string(): mk(&b),
    });

    let recipe = write(&dir, "two-sinks.json", recipe_json(nodes).as_bytes());
    let err = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        recipe.to_str().unwrap(),
        dir.join("o.bin").to_str().unwrap(),
    )
    .unwrap_err();
    assert!(err.contains("exactly one output node"));
}
