//! `text.reverse` — реверс байтов входа (не Unicode-safe: реверсирует
//! на уровне байтов, как `xxd | rev`).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct Reverse;

impl Transform for Reverse {
    fn id(&self) -> &'static str {
        "text.reverse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Reverse"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        Ok(Cow::Owned(input.as_ref().iter().rev().copied().collect()))
    }
}

inventory::submit! { crate::TransformEntry(&Reverse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn reverse_basic() {
        let ctx = NullExecutionContext;
        let out = Reverse.apply(Cow::Borrowed(b"abc"), &json!({}), &ctx).unwrap();
        assert_eq!(out.as_ref(), b"cba");
    }

    #[test]
    fn reverse_twice_is_identity() {
        let ctx = NullExecutionContext;
        let once = Reverse.apply(Cow::Borrowed(b"hello"), &json!({}), &ctx).unwrap();
        let twice = Reverse.apply(once, &json!({}), &ctx).unwrap();
        assert_eq!(twice.as_ref(), b"hello");
    }

    #[test]
    fn reverse_binary_bytes() {
        let ctx = NullExecutionContext;
        let out = Reverse
            .apply(Cow::Borrowed(&[1, 2, 3]), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), &[3, 2, 1]);
    }

    #[test]
    fn reverse_empty() {
        let ctx = NullExecutionContext;
        let out = Reverse.apply(Cow::Borrowed(b""), &json!({}), &ctx).unwrap();
        assert!(out.is_empty());
    }
}
