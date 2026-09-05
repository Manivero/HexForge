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
        &[input.to_str().unwrap().to_string()],
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
        &[recipe.to_str().unwrap().to_string()],
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
        &[recipe.to_str().unwrap().to_string()],
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
        &[recipe.to_str().unwrap().to_string()],
        dir.join("o.bin").to_str().unwrap(),
    )
    .unwrap_err();
    assert!(err.contains("exactly one output node"));
}

#[test]
fn run_recipe_multi_source_concat() {
    use hexforge_engine::graph_dto::OperationNodeDto;
    let dir = temp_dir();
    // Fixed UUIDs to ensure deterministic sorted order: a < b lexicographically
    let a = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
    let b = uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap();
    let c = NodeId::new_v4();
    let nodes = json!({
        a.to_string(): OperationNodeDto {
            id: a.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: json!({ "sourceHandle": "00000000-0000-4000-8000-000000000000" }),
            inputs: vec![],
        },
        b.to_string(): OperationNodeDto {
            id: b.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: json!({ "sourceHandle": "00000000-0000-4000-8000-000000000001" }),
            inputs: vec![],
        },
        c.to_string(): OperationNodeDto {
            id: c.to_string(),
            operation_id: "streaming.concat".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![a.to_string(), b.to_string()],
        }
    });
    let recipe = write(&dir, "concat.json", recipe_json(nodes).as_bytes());
    let in1 = write(&dir, "in1.bin", b"Hello");
    let in2 = write(&dir, "in2.bin", b"World");
    let out = dir.join("out.bin");
    let summary = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        &[
            in1.to_str().unwrap().to_string(),
            in2.to_str().unwrap().to_string(),
        ],
        out.to_str().unwrap(),
    )
    .expect("multi-source concat must run");
    // Hello -> Uryyb, World -> Jbeyq, concat -> UryybJbeyq (rot13 of each)
    let written = std::fs::read(&out).unwrap();
    assert_eq!(written, b"UryybJbeyq");
    assert_eq!(summary.executed_nodes, 3);
}

#[test]
fn run_recipe_multi_source_mismatched_in_count_rejected() {
    use hexforge_engine::graph_dto::OperationNodeDto;
    let dir = temp_dir();
    let a = NodeId::new_v4();
    let b = NodeId::new_v4();
    let c = NodeId::new_v4();
    let nodes = json!({
        a.to_string(): OperationNodeDto {
            id: a.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![],
        },
        b.to_string(): OperationNodeDto {
            id: b.to_string(),
            operation_id: "text.rot13".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![],
        },
        c.to_string(): OperationNodeDto {
            id: c.to_string(),
            operation_id: "streaming.concat".into(),
            operation_version: "1.0.0".into(),
            params: json!({}),
            inputs: vec![a.to_string(), b.to_string()],
        }
    });
    let recipe = write(&dir, "r.json", recipe_json(nodes).as_bytes());
    let in1 = write(&dir, "in1.bin", b"a");
    let out = dir.join("o.bin");
    // 1 input for 2 roots should broadcast and succeed (backward compat)
    let ok = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        &[in1.to_str().unwrap().to_string()],
        out.to_str().unwrap(),
    );
    assert!(
        ok.is_ok(),
        "1 input for 2 roots should broadcast and succeed, got err: {:?}",
        ok.err()
    );
    let in2 = write(&dir, "in2.bin", b"b");
    let in3 = write(&dir, "in3.bin", b"c");
    let err2 = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        &[
            in1.to_str().unwrap().to_string(),
            in2.to_str().unwrap().to_string(),
            in3.to_str().unwrap().to_string(),
        ],
        dir.join("o2.bin").to_str().unwrap(),
    )
    .unwrap_err();
    assert!(err2.contains("must be 1 or match number of source nodes"));
}

#[test]
fn run_recipe_preserves_root_params_when_binding_source() {
    use hexforge_engine::graph_dto::OperationNodeDto;
    let dir = temp_dir();
    let root_id = NodeId::new_v4();

    // Корень base64.decode с собственным params alphabet=url_safe: биндинг
    // --in обязан ДОБАВИТЬ sourceHandle, а не затереть params целиком —
    // иначе стандартный алфавит отвергнет "-_8=" как InvalidInput.
    let nodes = json!({
        root_id.to_string(): OperationNodeDto {
            id: root_id.to_string(),
            operation_id: "encoding.base64.decode".into(),
            operation_version: "1.0.0".into(),
            params: json!({ "alphabet": "url_safe" }),
            inputs: vec![],
        }
    });

    let recipe = write(&dir, "recipe.hexforge", recipe_json(nodes).as_bytes());
    let input = write(&dir, "in.bin", b"-_8=");
    let out = dir.join("out.bin");

    let summary = hexforge_cli::run_recipe(
        recipe.to_str().unwrap(),
        &[input.to_str().unwrap().to_string()],
        out.to_str().unwrap(),
    )
    .expect("url-safe base64 must decode when alphabet param is preserved");

    // "-_8=" в url_safe-алфавите = байты [0xFB, 0xFF].
    assert_eq!(summary.executed_nodes, 1);
    assert_eq!(summary.output_bytes, 2);
    assert_eq!(std::fs::read(&out).unwrap(), vec![0xFB, 0xFF]);
}
