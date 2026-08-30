//! hexforge-ops — встроенные реализации `Transform`.
//!
//! Регистрация через `inventory::submit!` вместо центрального `match`/списка:
//! добавление новой операции не требует правок в этом файле или в
//! `hexforge-core` — это прямое архитектурное следствие PRD FR-3.1
//! ("ядро ничего не знает о конкретных операциях") и требования расти
//! до 400+ операций без узкого места в одном файле.

pub mod binary;
pub mod compression;
pub mod crypto;
pub mod encoding;
pub mod hashing;
pub mod network;
pub mod streaming;
pub mod text;

use hexforge_core::{MergeTransform, Transform};

/// `inventory::collect!` реализует свой внутренний трейт для переданного типа —
/// по правилам сиротства (orphan rules) это обязано быть локальным типом
/// этого крейта, а не голым `&'static dyn Transform` (трейт `Transform`
/// определён в `hexforge-core`, чужой для `hexforge-ops`). Регистрация идёт
/// через тонкие локальные обёртки: унарные/базовые операции через
/// `TransformEntry`, N-арные слияния дополнительно через `MergeEntry`.
pub struct TransformEntry(pub &'static dyn Transform);

inventory::collect!(TransformEntry);

pub struct MergeEntry(pub &'static dyn MergeTransform);

inventory::collect!(MergeEntry);

/// Строит реестр из всех операций, собранных `inventory` на этапе линковки.
/// Вызывается один раз при старте процесса (Tauri `setup` hook или CLI `main`).
pub fn build_registry() -> hexforge_core::TransformRegistry {
    let mut registry = hexforge_core::TransformRegistry::new();
    for entry in inventory::iter::<TransformEntry> {
        registry.register(entry.0);
    }
    for entry in inventory::iter::<MergeEntry> {
        registry.register_merge(entry.0);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_builtin_operations() {
        let registry = build_registry();
        // Реестр должен содержать все встроенные операции (49+ на 2026-08, включая pbkdf2). Порог 50 ловит случайную потерю inventory-сбора.
        assert!(
            registry.len() >= 50,
            "expected at least 50 built-in operations, got {}",
            registry.len()
        );
        assert!(registry.get("encoding.base64.decode").is_some());
        assert!(registry.get("hashing.sha256").is_some());
        assert!(
            registry.get("hashing.pbkdf2").is_some(),
            "pbkdf2 must be registered"
        );
        assert!(registry.get("hashing.hmac").is_some());
        assert!(registry.get("text.case_transform").is_some());
    }
}
