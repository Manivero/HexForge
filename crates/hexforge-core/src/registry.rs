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

    /// Plugin-only registration (canonical `plugin:<id>` namespace).
    /// Fail-closed: a bare (unprefixed) id or a duplicate never overwrites —
    /// silent shadowing of builtins or sibling plugins is a reproducibility
    /// and security hole, so the caller must surface the error instead.
    pub fn register_plugin(&mut self, transform: &'static dyn Transform) -> Result<(), String> {
        let id = transform.id();
        if !id.starts_with("plugin:") {
            return Err(format!(
                "refusing to register plugin transform under bare id '{id}': \
                 plugin operations must use the canonical 'plugin:<manifest-id>' form"
            ));
        }
        if self.entries.contains_key(id) {
            return Err(format!(
                "duplicate plugin operation '{id}': already registered, refusing to overwrite"
            ));
        }
        self.entries.insert(id, transform);
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{
        ByteView, ExecutionContext, MemoryCost, TransformCapabilities, TransformError,
    };

    struct Fake {
        id: &'static str,
        version: &'static str,
        plugin: bool,
    }

    impl Transform for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn version(&self) -> &'static str {
            self.version
        }
        fn display_name(&self) -> &'static str {
            self.id
        }
        fn category(&self) -> &'static str {
            "Test"
        }
        fn capabilities(&self) -> TransformCapabilities {
            TransformCapabilities {
                deterministic: true,
                streamable: false,
                memory_cost: MemoryCost::FullBuffer,
            }
        }
        fn origin(&self) -> &'static str {
            if self.plugin {
                "plugin"
            } else {
                "builtin"
            }
        }
        fn apply<'a>(
            &self,
            input: ByteView<'a>,
            _params: &serde_json::Value,
            _ctx: &dyn ExecutionContext,
        ) -> Result<ByteView<'a>, TransformError> {
            Ok(input)
        }
    }

    fn leaked(id: &'static str, plugin: bool) -> &'static dyn Transform {
        let t: Box<dyn Transform> = Box::new(Fake {
            id,
            version: "1.0.0",
            plugin,
        });
        Box::leak(t)
    }

    #[test]
    fn register_plugin_accepts_canonical_prefixed_id() {
        let mut reg = TransformRegistry::new();
        let pt = leaked("plugin:test.op", true);
        assert!(reg.register_plugin(pt).is_ok());
        assert!(reg.get("plugin:test.op").is_some());
    }

    #[test]
    fn register_plugin_rejects_bare_id() {
        let mut reg = TransformRegistry::new();
        let err = reg.register_plugin(leaked("test.op", true)).unwrap_err();
        assert!(err.contains("bare"), "{err}");
        assert!(reg.get("test.op").is_none());
    }

    #[test]
    fn register_plugin_rejects_duplicate_without_overwrite() {
        let mut reg = TransformRegistry::new();
        let first = leaked("plugin:test.op", true);
        assert!(reg.register_plugin(first).is_ok());
        let err = reg
            .register_plugin(leaked("plugin:test.op", true))
            .unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
        // First registration intact (pointer identity, not silently swapped).
        assert!(std::ptr::eq(
            reg.get("plugin:test.op").unwrap(),
            first as &'static dyn Transform
        ));
    }

    #[test]
    fn plugin_id_cannot_shadow_builtin() {
        let mut reg = TransformRegistry::new();
        let builtin = leaked("encoding.base64.encode", false);
        reg.register(builtin);
        // Same manifest id lands in the plugin namespace, builtin untouched.
        assert!(reg
            .register_plugin(leaked("plugin:encoding.base64.encode", true))
            .is_ok());
        assert!(std::ptr::eq(
            reg.get("encoding.base64.encode").unwrap(),
            builtin as &'static dyn Transform
        ));
        assert!(reg.get("plugin:encoding.base64.encode").unwrap().origin() == "plugin");
    }
}
