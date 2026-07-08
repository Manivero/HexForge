//! Модель Node Graph — прямой ответ на структурное ограничение CyberChef
//! "recipe как линейный список" (см. `02-COMPETITIVE-GAP-ANALYSIS.md`, #2).
//! Граф — направленный ациклический (DAG): узел может иметь несколько
//! входов (merge/zip/xor-подобные операции) и несколько исходящих рёбер
//! (fork в несколько независимых веток).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

pub type NodeId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationNode {
    pub id: NodeId,
    /// Ссылается на запись в `TransformRegistry`, не хранит саму операцию.
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    /// 0 входов = узел-источник (файл, литерал). N>1 входов = merge-узел.
    pub inputs: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Graph {
    pub nodes: HashMap<NodeId, OperationNode>,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum GraphError {
    #[error("cycle detected in graph")]
    CycleDetected,
    #[error("node {0} references unknown input")]
    DanglingInput(NodeId),
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_node(&mut self, node: OperationNode) {
        self.nodes.insert(node.id, node);
    }

    /// Списки смежности "родитель → дети", построенные за один проход по
    /// узлам. Раньше `topo_order`/`downstream_of` вызывали приватный
    /// `direct_children`, который на КАЖДЫЙ узел заново сканировал ВСЕ узлы
    /// графа (`self.nodes.values().filter(...)`) — то есть несмотря на
    /// комментарий "O(V+E)" в доке, фактическая сложность обоих методов была
    /// O(V²): для 10 000 узлов это уже 100 млн проверок вместо ~20 000.
    /// Этот метод строит adjacency list один раз, и оба вызывающих метода
    /// используют готовую карту.
    fn build_children_map(&self) -> HashMap<NodeId, Vec<NodeId>> {
        let mut children: HashMap<NodeId, Vec<NodeId>> =
            self.nodes.keys().map(|id| (*id, Vec::new())).collect();
        for node in self.nodes.values() {
            for input in &node.inputs {
                if let Some(list) = children.get_mut(input) {
                    list.push(node.id);
                }
            }
        }
        children
    }

    /// Топологическая сортировка (алгоритм Кана), настоящий O(V+E).
    /// Возвращает `GraphError::CycleDetected`, если граф не является DAG,
    /// и `GraphError::DanglingInput`, если узел ссылается на несуществующий вход.
    pub fn topo_order(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut in_degree: HashMap<NodeId, usize> =
            self.nodes.keys().map(|id| (*id, 0)).collect();

        for node in self.nodes.values() {
            for input in &node.inputs {
                if !self.nodes.contains_key(input) {
                    return Err(GraphError::DanglingInput(node.id));
                }
            }
            *in_degree.entry(node.id).or_insert(0) = node.inputs.len();
        }

        let children = self.build_children_map();

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(id, _)| *id)
            .collect();

        let mut order = Vec::with_capacity(self.nodes.len());

        while let Some(node_id) = queue.pop_front() {
            order.push(node_id);
            for &child in children.get(&node_id).into_iter().flatten() {
                let deg = in_degree.get_mut(&child).expect("child must be tracked");
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(child);
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(GraphError::CycleDetected);
        }

        Ok(order)
    }

    /// Все узлы, достижимые из `node_id` по исходящим рёбрам (включая сам
    /// `node_id`). Используется для точечной инвалидации ("stale") при
    /// изменении параметров узла — HexForge не пересчитывает весь граф,
    /// только реально затронутую часть (FR-1.6).
    pub fn downstream_of(&self, node_id: NodeId) -> Vec<NodeId> {
        let children = self.build_children_map();
        let mut visited = HashSet::new();
        let mut stack = vec![node_id];
        let mut result = Vec::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            result.push(current);
            for &child in children.get(&current).into_iter().flatten() {
                if !visited.contains(&child) {
                    stack.push(child);
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: NodeId, inputs: Vec<NodeId>) -> OperationNode {
        OperationNode {
            id,
            operation_id: "test.noop".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            inputs,
        }
    }

    #[test]
    fn topo_order_linear_chain() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let mut g = Graph::new();
        g.insert_node(node(a, vec![]));
        g.insert_node(node(b, vec![a]));
        g.insert_node(node(c, vec![b]));

        let order = g.topo_order().unwrap();
        assert_eq!(order, vec![a, b, c]);
    }

    #[test]
    fn topo_order_detects_cycle() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut g = Graph::new();
        g.insert_node(node(a, vec![b]));
        g.insert_node(node(b, vec![a]));

        assert!(matches!(g.topo_order(), Err(GraphError::CycleDetected)));
    }

    #[test]
    fn topo_order_supports_merge_node() {
        // a -> c, b -> c  (c — merge-узел с двумя входами, напр. XOR двух веток)
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let mut g = Graph::new();
        g.insert_node(node(a, vec![]));
        g.insert_node(node(b, vec![]));
        g.insert_node(node(c, vec![a, b]));

        let order = g.topo_order().unwrap();
        let pos_a = order.iter().position(|x| *x == a).unwrap();
        let pos_b = order.iter().position(|x| *x == b).unwrap();
        let pos_c = order.iter().position(|x| *x == c).unwrap();
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn downstream_of_fork() {
        // a -> b, a -> c  (fork: два независимых потребителя одного источника)
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let mut g = Graph::new();
        g.insert_node(node(a, vec![]));
        g.insert_node(node(b, vec![a]));
        g.insert_node(node(c, vec![a]));

        let mut downstream = g.downstream_of(a);
        downstream.sort();
        let mut expected = vec![a, b, c];
        expected.sort();
        assert_eq!(downstream, expected);
    }

    #[test]
    fn dangling_input_detected() {
        let a = Uuid::new_v4();
        let missing = Uuid::new_v4();
        let mut g = Graph::new();
        g.insert_node(node(a, vec![missing]));

        assert!(matches!(g.topo_order(), Err(GraphError::DanglingInput(_))));
    }
}
