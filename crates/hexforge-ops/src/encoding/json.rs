use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct JsonPretty;

impl Transform for JsonPretty {
    fn id(&self) -> &'static str {
        "encoding.json.pretty"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "JSON Pretty"
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
            serde_json::from_slice(input.as_ref()).map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid JSON: {e}"),
            })?;
        let pretty = serde_json::to_string_pretty(&v)
            .map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

pub struct JsonMinify;

impl Transform for JsonMinify {
    fn id(&self) -> &'static str {
        "encoding.json.minify"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "JSON Minify"
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
            serde_json::from_slice(input.as_ref()).map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid JSON: {e}"),
            })?;
        let min = serde_json::to_string(&v).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(min.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&JsonPretty) }
inventory::submit! { crate::TransformEntry(&JsonMinify) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn pretty_and_minify_roundtrip() {
        let ctx = NullExecutionContext;
        let min = br#"{"a":1,"b":[2,3]}"#;
        let pretty = JsonPretty
            .apply(Cow::Borrowed(min), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(
            pretty.windows(2).any(|w| w == b"\n "),
            "pretty should contain newline+indent"
        );
        let back = JsonMinify
            .apply(pretty, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(back.as_ref(), min.as_slice());
    }

    #[test]
    fn rejects_invalid_json() {
        let ctx = NullExecutionContext;
        let err = JsonPretty
            .apply(Cow::Borrowed(b"{not json}"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
