use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct RemoveWhitespace;

impl Transform for RemoveWhitespace {
    fn id(&self) -> &'static str {
        "text.remove_whitespace"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Remove Whitespace"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: true,
            memory_cost: MemoryCost::PerChunk,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let out: Vec<u8> = input
            .as_ref()
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        Ok(Cow::Owned(out))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        _is_last: bool,
        _state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        Ok(chunk
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect())
    }
}

inventory::submit! { crate::TransformEntry(&RemoveWhitespace) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn removes_all_whitespace() {
        let ctx = NullExecutionContext;
        let out = RemoveWhitespace
            .apply(
                Cow::Borrowed(b" a b\tc\nd e "),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"abcde");
    }

    #[test]
    fn empty_input() {
        let ctx = NullExecutionContext;
        let out = RemoveWhitespace
            .apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"");
    }

    #[test]
    fn chunked_matches_apply() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});
        let data = b"a b\tc\nd e f  g";
        let whole = RemoveWhitespace
            .apply(Cow::Borrowed(data), &params, &ctx)
            .unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        for (i, part) in [b"a b".as_slice(), b"\tc\n", b"d e f", b"  g"]
            .iter()
            .enumerate()
        {
            chunked.extend_from_slice(
                &RemoveWhitespace
                    .apply_chunk(part, i == 3, &mut state, &params, &ctx)
                    .unwrap(),
            );
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }
}
