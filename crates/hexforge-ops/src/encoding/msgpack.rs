use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct MsgpackDecode;

impl Transform for MsgpackDecode {
    fn id(&self) -> &'static str {
        "encoding.msgpack.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "MessagePack Decode"
    }
    fn category(&self) -> &'static str {
        "Encoding"
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
        let v: serde_json::Value =
            rmp_serde::from_slice(input.as_ref()).map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid MessagePack: {e}"),
            })?;
        let pretty = serde_json::to_string_pretty(&v)
            .map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&MsgpackDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn decode_roundtrip() {
        let ctx = NullExecutionContext;
        let original = json!({"a": 1, "b": [2, 3], "c": "hello"});
        let msgpack = rmp_serde::to_vec(&original).unwrap();
        let out = MsgpackDecode
            .apply(Cow::Borrowed(&msgpack), &json!({}), &ctx)
            .unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn rejects_invalid_msgpack() {
        let ctx = NullExecutionContext;
        // 0xC1 is never used in MessagePack spec
        let err = MsgpackDecode
            .apply(Cow::Borrowed(&[0xC1]), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
