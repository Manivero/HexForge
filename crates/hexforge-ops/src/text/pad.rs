use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct Pad;

impl Transform for Pad {
    fn id(&self) -> &'static str {
        "text.pad"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Pad Text"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "length": { "type": "integer", "minimum": 0, "default": 0, "description": "Target length (bytes); if input longer, no change" },
                "char": { "type": "string", "default": " ", "description": "Single char to pad with (first char used)" },
                "side": { "type": "string", "enum": ["right", "left", "both"], "default": "right" }
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
        let length = params.get("length").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let pad_str = params.get("char").and_then(|v| v.as_str()).unwrap_or(" ");
        let ch = pad_str.chars().next().unwrap_or(' ');
        let side = params
            .get("side")
            .and_then(|v| v.as_str())
            .unwrap_or("right");

        let input_str = String::from_utf8_lossy(input.as_ref()).to_string();
        let input_len = input_str.chars().count();
        if input_len >= length {
            return Ok(Cow::Owned(input.into_owned()));
        }
        let pad_len = length - input_len;
        let out = match side {
            "left" => {
                let pad = ch.to_string().repeat(pad_len);
                let mut s = pad;
                s.push_str(&input_str);
                s
            }
            "both" => {
                let left = pad_len / 2;
                let right = pad_len - left;
                let mut s = ch.to_string().repeat(left);
                s.push_str(&input_str);
                s.push_str(&ch.to_string().repeat(right));
                s
            }
            _ => {
                let mut s = input_str;
                s.push_str(&ch.to_string().repeat(pad_len));
                s
            }
        };
        Ok(Cow::Owned(out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Pad) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn pad_right_default() {
        let ctx = NullExecutionContext;
        let out = Pad
            .apply(Cow::Borrowed(b"hi"), &serde_json::json!({"length":5}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"hi   ");
    }

    #[test]
    fn pad_left() {
        let ctx = NullExecutionContext;
        let out = Pad
            .apply(
                Cow::Borrowed(b"hi"),
                &serde_json::json!({"length":5,"side":"left","char":"x"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"xxxhi");
    }

    #[test]
    fn pad_both() {
        let ctx = NullExecutionContext;
        let out = Pad
            .apply(
                Cow::Borrowed(b"hi"),
                &serde_json::json!({"length":5,"side":"both","char":"-"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"-hi--"); // pad 3: left 1, right 2
    }

    #[test]
    fn no_pad_if_longer() {
        let ctx = NullExecutionContext;
        let out = Pad
            .apply(
                Cow::Borrowed(b"hello"),
                &serde_json::json!({"length":3}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"hello");
    }
}
