//! Зеркало `GraphDto` из `src/lib/ipc-contract.ts` (camelCase на проводе).
//! Используется IPC-слоем (`set_graph`, export/import recipe), CLI-режимом
//! (`run recipe --in --out`) и валидатором ниже: конвертация в доменный
//! `Graph` проверяет UUID'ы и связность входов, топологическая сортировка —
//! ацикличность (инвариант ядра).

use crate::error::{HexForgeError, HexForgeResult};
use hexforge_core::graph::{Graph, NodeId, OperationNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationNodeDto {
    pub id: String,
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDto {
    pub nodes: HashMap<String, OperationNodeDto>,
}

impl TryFrom<GraphDto> for Graph {
    type Error = HexForgeError;

    fn try_from(dto: GraphDto) -> Result<Self, Self::Error> {
        let mut graph = Graph::new();
        for (key, node) in dto.nodes {
            // Ключ мапы обязан совпадать с node.id: иначе два ключа с одним
            // id тихо перезаписали бы друг друга в Graph.nodes (потеря узлов),
            // а FE↔BE адресация разъехалась бы без единой ошибки.
            if key != node.id {
                return Err(HexForgeError::invalid_input(format!(
                    "node map key '{key}' does not match node.id '{}'",
                    node.id
                )));
            }
            let node_id: NodeId = parse_uuid(&node.id)?;
            let inputs = node
                .inputs
                .iter()
                .map(|s| parse_uuid(s))
                .collect::<Result<Vec<_>, _>>()?;
            graph.insert_node(OperationNode {
                id: node_id,
                operation_id: node.operation_id,
                operation_version: node.operation_version,
                params: node.params,
                inputs,
            });
        }
        Ok(graph)
    }
}

fn parse_uuid(raw: &str) -> Result<NodeId, HexForgeError> {
    Uuid::parse_str(raw)
        .map_err(|_| HexForgeError::invalid_input(format!("'{raw}' is not a valid node id")))
}

/// Структурно-семантическая валидация рецепта/графа против реестра:
/// UUID'ы, DAG и воспроизводимость операций (наличие + точная версия, FR-4.2).
/// Возвращает доменный граф для дальнейшего исполнения.
pub fn validate_graph(
    dto: GraphDto,
    registry: &hexforge_core::TransformRegistry,
) -> HexForgeResult<Graph> {
    let graph: Graph = dto.try_into()?;
    graph.topo_order().map_err(HexForgeError::from)?;

    let mut missing: Vec<String> = Vec::new();
    for node in graph.nodes.values() {
        let reproducible = registry
            .get(&node.operation_id)
            .map(|t| t.version() == node.operation_version)
            .unwrap_or(false);
        if !reproducible && !missing.contains(&node.operation_id) {
            missing.push(node.operation_id.clone());
        }
    }
    if !missing.is_empty() {
        return Err(HexForgeError::invalid_input(format!(
            "operations missing from registry or version-mismatched: {}",
            missing.join(", ")
        )));
    }
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto_node(id: &str) -> OperationNodeDto {
        OperationNodeDto {
            id: id.to_string(),
            operation_id: "text.rot13".to_string(),
            operation_version: "1.0.0".to_string(),
            params: serde_json::json!({}),
            inputs: vec![],
        }
    }

    #[test]
    fn graph_dto_rejects_key_id_mismatch() {
        let real = "00000000-0000-4000-8000-000000000001";
        let mut nodes = HashMap::new();
        // Два разных ключа с одним node.id раньше тихо схлопывались
        // в один узел (потеря данных рецепта без ошибки).
        nodes.insert("other-key".to_string(), dto_node(real));
        nodes.insert(real.to_string(), dto_node(real));
        let err = Graph::try_from(GraphDto { nodes }).unwrap_err();
        assert!(
            err.message.contains("does not match node.id"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn graph_dto_accepts_matching_key_and_id() {
        let real = "00000000-0000-4000-8000-000000000001";
        let mut nodes = HashMap::new();
        nodes.insert(real.to_string(), dto_node(real));
        let graph = Graph::try_from(GraphDto { nodes }).expect("matching key/id must convert");
        assert_eq!(graph.nodes.len(), 1);
    }
}
