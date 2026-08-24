//! Реестр операций — единственная точка, через которую граф (по `operation_id`)
//! находит фактическую реализацию `Transform`. Ядро не хранит компилируемый
//! список операций (это была бы центральная бутылочная горлышко при росте
//! до 400+ операций, см. PRD §3.3) — заполнение реестра происходит на
//! старте процесса в `hexforge-ops` (через `inventory::submit!`) и
//! в `hexforge-plugin-host` (динамически, из WASM-модулей).

use crate::transform::{MergeTransform, Transform};
use std::collections::HashMap;

#[derive(Default)]
pub struct TransformRegistry {
    entries: HashMap<&'static str, &'static dyn Transform>,
    /// N-арные операции слияния (PRD FR-1.4). Отдельная карта, а не даункаст:
    /// трейт-объекты без Any-хаков, регистрация только для операций,
    /// реализующих `MergeTransform`.
    merges: HashMap<&'static str, &'static dyn MergeTransform>,
}

impl TransformRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, transform: &'static dyn Transform) {
        self.entries.insert(transform.id(), transform);
    }

    pub fn register_merge(&mut self, transform: &'static dyn MergeTransform) {
        self.merges.insert(transform.id(), transform);
    }

    pub fn get(&self, operation_id: &str) -> Option<&'static dyn Transform> {
        self.entries.get(operation_id).copied()
    }

    /// Merge-реализация операции; `None` = операция унарная и узел с
    /// несколькими входами на ней неисполним.
    pub fn get_merge(&self, operation_id: &str) -> Option<&'static dyn MergeTransform> {
        self.merges.get(operation_id).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = &'static dyn Transform> + '_ {
        self.entries.values().copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
