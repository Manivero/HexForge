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
    in_path: &str,
    out_path: &str,
) -> Result<RunSummary, String> {
    validate_cli_path(recipe_path, "recipe")?;
    validate_cli_path(in_path, "input")?;
    validate_cli_path(out_path, "output")?;
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

    // 3. Корни: узлы без входов. Все получают один и тот же --in источник
    //    (MVP-семантика: мультиисточниковые рецепты пока не поддержаны).
    let roots: Vec<NodeId> = graph
        .nodes
        .values()
        .filter(|n| n.inputs.is_empty())
        .map(|n| n.id)
        .collect();
    if roots.is_empty() {
        return Err("recipe has no source nodes (nodes without inputs)".into());
    }

    // NFR-2: для файлов >16MiB используем mmap вместо полной загрузки в RAM (constant memory)
    // Для маленьких файлов оставляем InMemory (быстрее, проще). Fallback на read если mmap не удался
    // (например, пустой файл на Windows или pipe).
    let input_handle = {
        let file = std::fs::File::open(in_path)
            .map_err(|e| format!("cannot open input '{in_path}': {e}"))?;
        let meta = file
            .metadata()
            .map_err(|e| format!("cannot stat input '{in_path}': {e}"))?;
        if meta.len() > 16 * 1024 * 1024 {
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
        }
    };
    {
        let handle = input_handle;
        for root_id in &roots {
            // ВАЖНО: read-guard ограничен блоком; if let со скрутини-временем
            // протянул бы его через graph.write() ниже → дедлок на том же треде.
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
    let graph = hexforge_engine::graph_dto::validate_graph(dto, &registry)
        .map_err(|e| e.message)?;

    Ok(format!(
        "recipe valid: {} node(s), {} operation(s) in registry",
        graph.nodes.len(),
        registry.len()
    ))
}
