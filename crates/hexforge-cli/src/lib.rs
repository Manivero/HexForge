//! hexforge-cli — headless-режим HexForge (PRD FR-7.3): запуск рецепта
//! `.hexforge` (формат = GraphDto, тот же, что пишет export_recipe) над
//! входным файлом с записью результата. Тот же движок, что и GUI:
//! hexforge-engine + hexforge-ops.
//!
//! ```text
//! hexforge-cli run recipe.hexforge --in input.bin --out output.bin
//! ```

use hexforge_core::graph::NodeId;
use hexforge_engine::graph_dto::{validate_graph, GraphDto};
use hexforge_engine::state::{AppState, SourceEntry};
use serde_json::json;
use std::collections::HashSet;

/// Итог успешного запуска рецепта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub output_bytes: u64,
    pub duration_ms: u64,
    /// Число исполненных узлов цепочки (по журналу истории).
    pub executed_nodes: usize,
}

fn validate_cli_path(path: &str, field: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("{field} path must not be empty"));
    }
    if path.len() > 4096 {
        return Err(format!("{field} path exceeds maximum length (4096)"));
    }
    if path.contains('\0') {
        return Err(format!("{field} path contains null byte"));
    }
    Ok(())
}

/// Ошибки CLI: человекочитаемая строка уходит в stderr / тестовый assert.
pub fn run_recipe(
    recipe_path: &str,
    in_paths: &[String],
    out_path: &str,
) -> Result<RunSummary, String> {
    validate_cli_path(recipe_path, "recipe")?;
    for p in in_paths {
        validate_cli_path(p, "input")?;
    }
    validate_cli_path(out_path, "output")?;
    if in_paths.is_empty() {
        return Err("at least one --in <file> is required".into());
    }
    let started = std::time::Instant::now();

    // 1. Рецепт: JSON формата GraphDto (контракт 05-IPC).
    let text = std::fs::read_to_string(recipe_path)
        .map_err(|e| format!("cannot read recipe '{recipe_path}': {e}"))?;
    let dto: GraphDto = serde_json::from_str(&text)
        .map_err(|e| format!("'{recipe_path}' is not a valid recipe file: {e}"))?;

    // 2. Валидация против реестра встроенных операций (UUID/DAG/версии)
    //    и загрузка графа в состояние исполнения.
    let registry = hexforge_ops::build_registry();
    let graph = validate_graph(dto, &registry).map_err(|e| e.message)?;
    let state = AppState::new(registry);
    {
        let mut g = state.graph.write();
        for node in graph.nodes.values() {
            g.insert_node(node.clone());
        }
    }

    // 3. Корни: узлы без входов. Поддержка N источников (multi-source).
    let mut roots: Vec<NodeId> = graph
        .nodes
        .values()
        .filter(|n| n.inputs.is_empty())
        .map(|n| n.id)
        .collect();
    if roots.is_empty() {
        return Err("recipe has no source nodes (nodes without inputs)".into());
    }
    // Детерминированный порядок корней — сортировка по UUID-строке, чтобы
    // `--in file1 --in file2` маппилось стабильно независимо от HashMap-порядка.
    roots.sort_by_key(|a| a.to_string());

    // Проверка соответствия числа --in и числа корней
    if in_paths.len() != 1 && in_paths.len() != roots.len() {
        return Err(format!(
            "number of --in files ({}) must be 1 or match number of source nodes ({})",
            in_paths.len(),
            roots.len()
        ));
    }

    // Создаём SourceEntry для каждого --in (mmap >16MiB, иначе InMemory)
    let handles: Vec<uuid::Uuid> = {
        let mut hs = Vec::with_capacity(if in_paths.len() == 1 { 1 } else { roots.len() });
        let paths_to_create: Vec<&String> = if in_paths.len() == 1 {
            vec![&in_paths[0]]
        } else {
            in_paths.iter().collect()
        };
        for in_path in paths_to_create {
            let file = std::fs::File::open(in_path)
                .map_err(|e| format!("cannot open input '{in_path}': {e}"))?;
            let meta = file
                .metadata()
                .map_err(|e| format!("cannot stat input '{in_path}': {e}"))?;
            let handle = if meta.len() > 16 * 1024 * 1024 {
                match unsafe { memmap2::Mmap::map(&file) } {
                    Ok(mmap) => {
                        let mut sources = state.sources.write();
                        sources.insert(SourceEntry::Mapped(mmap))
                    }
                    Err(_) => {
                        let bytes = std::fs::read(in_path)
                            .map_err(|e| format!("cannot read input '{in_path}': {e}"))?;
                        let mut sources = state.sources.write();
                        sources.insert(SourceEntry::InMemory(bytes))
                    }
                }
            } else if meta.len() == 0 {
                let mut sources = state.sources.write();
                sources.insert(SourceEntry::InMemory(Vec::new()))
            } else {
                let bytes = std::fs::read(in_path)
                    .map_err(|e| format!("cannot read input '{in_path}': {e}"))?;
                let mut sources = state.sources.write();
                sources.insert(SourceEntry::InMemory(bytes))
            };
            hs.push(handle);
        }
        hs
    };

    // Привязываем handles к корням: 1 handle → все корни, N handles → N корней по порядку
    {
        for (idx, root_id) in roots.iter().enumerate() {
            let handle = if handles.len() == 1 {
                handles[0]
            } else {
                handles[idx]
            };
            let existing = { state.graph.read().nodes.get(root_id).cloned() };
            if let Some(mut node) = existing {
                node.params = json!({ "sourceHandle": handle.to_string() });
                state.graph.write().insert_node(node);
            }
        }
    }

    // 4. Стоки: узлы, не потребляемые никем. Ровно один — результат рецепта.
    let consumed: HashSet<NodeId> = graph
        .nodes
        .values()
        .flat_map(|n| n.inputs.iter().copied())
        .collect();
    let sinks: Vec<NodeId> = graph
        .nodes
        .keys()
        .copied()
        .filter(|id| !consumed.contains(id))
        .collect();
    if sinks.len() != 1 {
        return Err(format!(
            "recipe must have exactly one output node, found {}: {:?}",
            sinks.len(),
            sinks.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        ));
    }
    let sink = sinks[0];

    // 5. Исполнение через общий с GUI планировщик; прогресс — в stderr.
    let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output = hexforge_engine::scheduler::execute_chain(&state, &sink, &token, &|event| {
        eprintln!("[progress] {} {}B", event.node_id, event.bytes_processed);
    })
    .map_err(|e| format!("{:?}: {}", e.kind, e.message))?;

    // 6. Запись результата.
    std::fs::write(out_path, output.as_slice())
        .map_err(|e| format!("cannot write output '{out_path}': {e}"))?;

    let executed_nodes = state.history.read().order.len();

    Ok(RunSummary {
        output_bytes: output.len() as u64,
        duration_ms: started.elapsed().as_millis() as u64,
        executed_nodes,
    })
}
/// Валидация рецепта без запуска: проверяет JSON, UUID, DAG, наличие
/// операций в реестре и соответствие версий. Для CI-пайплайнов (FR-7.3).
pub fn validate_recipe(recipe_path: &str) -> Result<String, String> {
    validate_cli_path(recipe_path, "recipe")?;
    let text = std::fs::read_to_string(recipe_path)
        .map_err(|e| format!("cannot read recipe '{recipe_path}': {e}"))?;
    let dto: hexforge_engine::graph_dto::GraphDto = serde_json::from_str(&text)
        .map_err(|e| format!("'{recipe_path}' is not a valid recipe file: {e}"))?;

    let registry = hexforge_ops::build_registry();
    let graph =
        hexforge_engine::graph_dto::validate_graph(dto, &registry).map_err(|e| e.message)?;

    Ok(format!(
        "recipe valid: {} node(s), {} operation(s) in registry",
        graph.nodes.len(),
        registry.len()
    ))
}
