use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use regex::Regex;
use std::borrow::Cow;

pub struct RegexExtract;

impl Transform for RegexExtract {
    fn id(&self) -> &'static str {
        "text.regex_extract"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Regex Extract"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern (Rust regex syntax)" }
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
        let pattern = params
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "pattern".into(),
                reason: "string parameter 'pattern' is required".into(),
            })?;
        let re = Regex::new(pattern).map_err(|e| TransformError::InvalidParameter {
            field: "pattern".into(),
            reason: format!("invalid regex: {e}"),
        })?;
        let text = String::from_utf8_lossy(input.as_ref());
        let mut out = String::new();
        for m in re.find_iter(&text) {
            out.push_str(m.as_str());
            out.push('\n');
        }
        Ok(Cow::Owned(out.into_bytes()))
    }
}

pub struct RegexReplace;

impl Transform for RegexReplace {
    fn id(&self) -> &'static str {
        "text.regex_replace"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Regex Replace"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["pattern", "replacement"],
            "properties": {
                "pattern": { "type": "string" },
                "replacement": { "type": "string", "description": "Replacement string ($1, $name)" }
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
        let pattern = params
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "pattern".into(),
                reason: "string parameter 'pattern' is required".into(),
            })?;
        let replacement = params
            .get("replacement")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "replacement".into(),
                reason: "string parameter 'replacement' is required".into(),
            })?;
        let re = Regex::new(pattern).map_err(|e| TransformError::InvalidParameter {
            field: "pattern".into(),
            reason: format!("invalid regex: {e}"),
        })?;
        let text = String::from_utf8_lossy(input.as_ref());
        let replaced = re.replace_all(&text, replacement);
        Ok(Cow::Owned(replaced.into_owned().into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&RegexExtract) }
inventory::submit! { crate::TransformEntry(&RegexReplace) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn extract_basic() {
        let ctx = NullExecutionContext;
        let out = RegexExtract
            .apply(
                Cow::Borrowed(b"abc 123 def 456"),
                &serde_json::json!({"pattern": r"\d+"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"123\n456\n");
    }

    #[test]
    fn replace_basic() {
        let ctx = NullExecutionContext;
        let out = RegexReplace
            .apply(
                Cow::Borrowed(b"hello world"),
                &serde_json::json!({"pattern": "world", "replacement": "Rust"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"hello Rust");
    }

    #[test]
    fn replace_with_capture() {
        let ctx = NullExecutionContext;
        let out = RegexReplace
            .apply(
                Cow::Borrowed(b"2024-01-15"),
                &serde_json::json!({"pattern": r"(\d+)-(\d+)-(\d+)", "replacement": "$3/$2/$1"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"15/01/2024");
    }

    #[test]
    fn invalid_pattern_rejected() {
        let ctx = NullExecutionContext;
        let err = RegexExtract
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"pattern": "["}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn missing_params_rejected() {
        let ctx = NullExecutionContext;
        let err = RegexExtract
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
