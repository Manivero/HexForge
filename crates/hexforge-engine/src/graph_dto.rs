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
        for (_id, node) in dto.nodes {
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
pub fn validate_graph(dto: GraphDto, registry: &hexforge_core::TransformRegistry) -> HexForgeResult<Graph> {
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
