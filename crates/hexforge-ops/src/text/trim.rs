use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct Trim;

impl Transform for Trim {
    fn id(&self) -> &'static str {
        "text.trim"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Trim Whitespace"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": ["both", "start", "end"], "default": "both" }
            }
        })
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
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mode = params
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("both");
        let s = String::from_utf8_lossy(input.as_ref());
        let out = match mode {
            "start" => s.trim_start().to_string(),
            "end" => s.trim_end().to_string(),
            "both" => s.trim().to_string(),
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "mode".into(),
                    reason: "mode must be both|start|end".into(),
                })
            }
        };
        Ok(Cow::Owned(out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Trim) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn trim_both() {
        let ctx = NullExecutionContext;
        let out = Trim
            .apply(
                Cow::Borrowed(b"  hello  "),
                &serde_json::json!({"mode":"both"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"hello");
    }

    #[test]
    fn trim_start() {
        let ctx = NullExecutionContext;
        let out = Trim
            .apply(
                Cow::Borrowed(b"  hello  "),
                &serde_json::json!({"mode":"start"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"hello  ");
    }

    #[test]
    fn trim_end() {
        let ctx = NullExecutionContext;
        let out = Trim
            .apply(
                Cow::Borrowed(b"  hello  "),
                &serde_json::json!({"mode":"end"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"  hello");
    }
}
