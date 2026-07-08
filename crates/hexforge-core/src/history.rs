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
        format!(
            "{}@{}::{}::{}",
            self.operation_id,
            self.operation_version,
            self.input_content_hash.to_hex(),
            self.params
        )
    }
}

/// История — плоское хранилище снапшотов с явными родительскими ссылками,
/// что и формирует DAG (в отличие от `Vec<Action>` в CyberChef). Ветвление
/// из произвольной точки — это просто создание нового снапшота с
/// `parent = Some(любой_существующий_id)`, а не обязательно "последний".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub snapshots: std::collections::HashMap<SnapshotId, Snapshot>,
    pub current: Option<SnapshotId>,
}

impl History {
    pub fn record(&mut self, snapshot: Snapshot) {
        self.current = Some(snapshot.id);
        self.snapshots.insert(snapshot.id, snapshot);
    }

    /// Путь от корня до данного снапшота — последовательность операций,
    /// применение которых воспроизводит его состояние с нуля.
    pub fn lineage(&self, id: SnapshotId) -> Vec<&Snapshot> {
        let mut chain = Vec::new();
        let mut cursor = Some(id);
        while let Some(current_id) = cursor {
            let Some(snap) = self.snapshots.get(&current_id) else {
                break;
            };
            chain.push(snap);
            cursor = snap.parent;
        }
        chain.reverse();
        chain
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

    pub fn serialize<S: Serializer>(
        hash: &Option<blake3::Hash>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match hash {
            Some(h) => s.serialize_str(&h.to_hex()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<blake3::Hash>, D::Error> {
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
}
