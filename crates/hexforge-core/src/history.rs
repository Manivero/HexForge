//! Time-Travel History — история как DAG состояний, а не линейный undo/redo
//! стек (см. `02-COMPETITIVE-GAP-ANALYSIS.md`, #6). Снапшот не хранит сами
//! байты результата напрямую: он декларирует воспроизводимый рецепт
//! ("этот input_content_hash + эта операция@версия + эти params дают этот
//! результат"), а фактические байты живут в отдельном content-addressed
//! кэше, который может быть вытеснен под memory pressure без потери
//! воспроизводимости (FR-4.2).

use crate::graph::NodeId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SnapshotId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub parent: Option<SnapshotId>,
    pub node_id: NodeId,
    pub operation_id: String,
    pub operation_version: String,
    pub params: serde_json::Value,
    #[serde(with = "hash_hex")]
    pub input_content_hash: blake3::Hash,
    #[serde(with = "option_hash_hex")]
    pub output_content_hash: Option<blake3::Hash>,
}

impl Snapshot {
    /// Ключ воспроизводимости: два снапшота с одинаковым ключом гарантированно
    /// дают одинаковый результат (при условии `capabilities().deterministic == true`
    /// для соответствующей операции) — используется для дедупликации кэша.
    pub fn reproducibility_key(&self) -> String {
        reproducibility_key(
            &self.operation_id,
            &self.operation_version,
            &self.input_content_hash.to_hex()[..],
            &self.params,
        )
    }
}

/// Свободная функция ключа воспроизводимости — единый формат для снапшотов и
/// content-addressed кэша планировщика: планировщик считает хэш входа один раз
/// и использует его и для ключа кэша, и для записи снапшота (без повторного
/// хеширования). Формат: `op@version :: input_hex :: params` (serde_json Value
/// печатает детерминированно — ключи объектов отсортированы).
pub fn reproducibility_key(
    operation_id: &str,
    operation_version: &str,
    input_hash_hex: &str,
    params: &serde_json::Value,
) -> String {
    format!(
        "{}@{}::{}::{}",
        operation_id, operation_version, input_hash_hex, params
    )
}

/// История — плоское хранилище снапшотов с явными родительскими ссылками,
/// что и формирует DAG (в отличие от `Vec<Action>` в CyberChef). Ветвление
/// из произвольной точки — это просто создание нового снапшота с
/// `parent = Some(любой_существующий_id)`, а не обязательно "последний".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub snapshots: std::collections::HashMap<SnapshotId, Snapshot>,
    /// Порядок записи снапшотов — единственный источник детерминированного
    /// порядка для `list_snapshots` (итерация HashMap неупорядочена).
    /// Не является структурой ветвления: ветвление выражено только полями
    /// `parent`, `order` — хронология журнала.
    pub order: Vec<SnapshotId>,
    /// Индекс воспроизводимости: key → снапшот. Повторное состояние
    /// (тот же op@ver :: input :: params) НЕ порождает новый узел DAG —
    /// история остаётся графом уникальных состояний (FR-4.2).
    #[serde(default)]
    pub by_key: std::collections::HashMap<String, SnapshotId>,
    pub current: Option<SnapshotId>,
}

impl History {
    pub fn record(&mut self, snapshot: Snapshot) {
        let id = snapshot.id;
        let key = snapshot.reproducibility_key();
        self.current = Some(id);
        if self.snapshots.insert(id, snapshot).is_none() {
            self.order.push(id);
            self.by_key.insert(key, id);
        }
    }

    /// Существующий снапшот с идентичным ключом воспроизводимости, если есть.
    pub fn find_by_key(&self, key: &str) -> Option<SnapshotId> {
        self.by_key.get(key).copied()
    }

    /// Путь от корня до данного снапшота — последовательность операций,
    /// применение которых воспроизводит его состояние с нуля.
    /// Защищена от зацикливания: некорректное состояние с циклом в `parent`
    /// (возможно только при ручном конструировании/повреждении данных, т.к.
    /// `record` сам ничего не проверяет) обрывается на первом повторном
    /// посещении вместо бесконечного цикла.
    pub fn lineage(&self, id: SnapshotId) -> Vec<&Snapshot> {
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut cursor = Some(id);
        while let Some(current_id) = cursor {
            if !visited.insert(current_id) {
                break;
            }
            let Some(snap) = self.snapshots.get(&current_id) else {
                break;
            };
            chain.push(snap);
            cursor = snap.parent;
        }
        chain.reverse();
        chain
    }

    /// Снапшоты в порядке записи (`order`), пропуская отсутствующие id —
    /// используется IPC-слоем для стабильного рендера списка истории.
    pub fn ordered_snapshots(&self) -> Vec<&Snapshot> {
        self.order
            .iter()
            .filter_map(|id| self.snapshots.get(id))
            .collect()
    }
}

mod hash_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(hash: &blake3::Hash, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hash.to_hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<blake3::Hash, D::Error> {
        let hex = String::deserialize(d)?;
        hex.parse().map_err(serde::de::Error::custom)
    }
}

mod option_hash_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(hash: &Option<blake3::Hash>, s: S) -> Result<S::Ok, S::Error> {
        match hash {
            Some(h) => s.serialize_str(&h.to_hex()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<blake3::Hash>, D::Error> {
        let opt = Option::<String>::deserialize(d)?;
        opt.map(|hex| hex.parse().map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_reconstructs_path() {
        let mut history = History::default();
        let node_id = NodeId::new_v4();

        let root = Snapshot {
            id: Uuid::new_v4(),
            parent: None,
            node_id,
            operation_id: "encoding.base64.decode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            input_content_hash: blake3::hash(b"root"),
            output_content_hash: None,
        };
        let root_id = root.id;
        history.record(root);

        let child = Snapshot {
            id: Uuid::new_v4(),
            parent: Some(root_id),
            node_id,
            operation_id: "compression.gzip.decompress".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            input_content_hash: blake3::hash(b"child"),
            output_content_hash: None,
        };
        let child_id = child.id;
        history.record(child);

        let lineage = history.lineage(child_id);
        assert_eq!(lineage.len(), 2);
        assert_eq!(lineage[0].id, root_id);
        assert_eq!(lineage[1].id, child_id);
    }

    #[test]
    fn lineage_terminates_on_parent_cycle() {
        // Повреждённое/некорректно сконструированное состояние: a.parent = b,
        // b.parent = a. `lineage` обязан завершиться конечным обходом, а не
        // зациклиться навсегда.
        let mut history = History::default();
        let a_id = Uuid::new_v4();
        let b_id = Uuid::new_v4();

        let mut a = snapshot_for_test(a_id, None);
        a.parent = Some(b_id);
        let b = snapshot_for_test(b_id, Some(a_id));
        history.record(a);
        history.record(b);

        let lineage = history.lineage(a_id);
        assert!(!lineage.is_empty());
        assert!(lineage.len() <= 2, "cycle must terminate the walk");
    }

    #[test]
    fn lineage_of_unknown_id_is_empty() {
        let history = History::default();
        assert!(history.lineage(Uuid::new_v4()).is_empty());
    }

    #[test]
    fn record_maintains_insertion_order() {
        let mut history = History::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        history.record(snapshot_for_test(first, None));
        history.record(snapshot_for_test(second, Some(first)));

        assert_eq!(history.order, vec![first, second]);
        let ordered: Vec<SnapshotId> = history.ordered_snapshots().iter().map(|s| s.id).collect();
        assert_eq!(ordered, vec![first, second]);

        // Перезапись существующего id не должна дублировать запись в order.
        history.record(snapshot_for_test(first, None));
        assert_eq!(history.order, vec![first, second]);
        assert_eq!(history.snapshots.len(), 2);
        assert_eq!(history.current, Some(first));
    }

    /// Минимальный валидный снапшот для тестов структуры History.
    fn snapshot_for_test(id: SnapshotId, parent: Option<SnapshotId>) -> Snapshot {
        Snapshot {
            id,
            parent,
            node_id: NodeId::new_v4(),
            operation_id: "encoding.base64.encode".into(),
            operation_version: "1.0.0".into(),
            params: serde_json::json!({}),
            input_content_hash: blake3::hash(b"input"),
            output_content_hash: Some(blake3::hash(b"output")),
        }
    }
}
