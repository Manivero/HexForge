//! `streaming.concat` — первая N-арная операция слияния (PRD FR-1.4).
//! Склеивает входы в порядке их объявления в узле графа; детерминирована,
//! что делает результат воспроизводимым через content-hash (FR-4.2).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, MergeTransform, Transform, TransformCapabilities,
    TransformError,
};
use std::borrow::Cow;

pub struct ConcatMerge;

impl Transform for ConcatMerge {
    fn id(&self) -> &'static str {
        "streaming.concat"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Concatenate Inputs"
    }
    fn category(&self) -> &'static str {
        "Streaming"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false, // MVP: полный буфер; чанковый concat — вместе с cross-node pipelining
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn apply<'a>(
        &self,
        _input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        // Прямой apply для concat не определён: операция осмыслена только
        // как N-арная (планировщик вызывает apply_merge). Ошибка намеренно
        // диагностическая — попадание сюда означает баг вызова.
        Err(TransformError::Internal(
            "streaming.concat is a merge operation; it is executed via apply_merge with N inputs".into(),
        ))
    }
}

impl MergeTransform for ConcatMerge {
    fn apply_merge<'a>(
        &self,
        inputs: Vec<ByteView<'a>>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        if inputs.is_empty() {
            return Err(TransformError::InvalidInput {
                reason: "streaming.concat requires at least one input".into(),
            });
        }
        let total = inputs.iter().map(|i| i.len()).sum();
        let mut out = Vec::with_capacity(total);
        for input in &inputs {
            out.extend_from_slice(input.as_ref());
        }
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&ConcatMerge) }
inventory::submit! { crate::MergeEntry(&ConcatMerge) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use std::borrow::Cow;

    #[test]
    fn concat_preserves_input_order() {
        let ctx = NullExecutionContext;
        let out = ConcatMerge
            .apply_merge(
                vec![
                    Cow::Borrowed(&[0xDE, 0xAD]),
                    Cow::Borrowed(b"forge"),
                    Cow::Borrowed(&[]),
                ],
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), &[0xDE, 0xAD, b'f', b'o', b'r', b'g', b'e']);
    }

    #[test]
    fn single_input_is_passthrough() {
        let ctx = NullExecutionContext;
        let out = ConcatMerge
            .apply_merge(vec![Cow::Borrowed(b"solo")], &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"solo");
    }

    #[test]
    fn empty_inputs_rejected() {
        let ctx = NullExecutionContext;
        let err = ConcatMerge
            .apply_merge(vec![], &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn direct_apply_is_rejected_by_design() {
        let ctx = NullExecutionContext;
        let err = ConcatMerge
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::Internal(_)));
    }

    #[test]
    fn registered_in_build_registry_maps() {
        let registry = crate::build_registry();
        assert!(registry.get("streaming.concat").is_some());
        assert!(
            registry.get_merge("streaming.concat").is_some(),
            "merge map must resolve streaming.concat"
        );
    }
}
