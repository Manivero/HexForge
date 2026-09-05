use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;
use unicode_normalization::UnicodeNormalization;

pub struct UnicodeNormalize;

impl Transform for UnicodeNormalize {
    fn id(&self) -> &'static str {
        "text.unicode_normalize"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Unicode Normalize"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["form"],
            "properties": {
                "form": { "type": "string", "enum": ["nfc", "nfd", "nfkc", "nfkd"], "default": "nfc" }
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
        // Schema объявляет default="nfc", но движок дефолты не инжектит
        // (дефолты — подсказка UI-формы); по конвенции кодовой базы
        // (напр. base64 alphabet → STANDARD) отсутствие = дефолт.
        let form = params.get("form").and_then(|v| v.as_str()).unwrap_or("nfc");
        let s = String::from_utf8_lossy(input.as_ref());
        let out = match form {
            "nfc" => s.nfc().collect::<String>(),
            "nfd" => s.nfd().collect::<String>(),
            "nfkc" => s.nfkc().collect::<String>(),
            "nfkd" => s.nfkd().collect::<String>(),
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "form".into(),
                    reason: format!("unknown form '{form}'; expected nfc|nfd|nfkc|nfkd"),
                })
            }
        };
        Ok(Cow::Owned(out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&UnicodeNormalize) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn nfc_composes() {
        let ctx = NullExecutionContext;
        // e + combining acute (U+0301) vs precomposed é (U+00E9)
        let decomposed = "e\u{0301}";
        let out = UnicodeNormalize
            .apply(
                Cow::Borrowed(decomposed.as_bytes()),
                &serde_json::json!({"form":"nfc"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), "é".as_bytes());
    }

    #[test]
    fn nfd_decomposes() {
        let ctx = NullExecutionContext;
        let out = UnicodeNormalize
            .apply(
                Cow::Borrowed("é".as_bytes()),
                &serde_json::json!({"form":"nfd"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), "e\u{0301}".as_bytes());
    }

    #[test]
    fn invalid_form_rejected() {
        let ctx = NullExecutionContext;
        let err = UnicodeNormalize
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"form":"bad"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn missing_form_uses_schema_default_nfc() {
        // Schema: required + default="nfc"; движок дефолты не инжектит,
        // поэтому отсутствие обязано вести себя как nfc, а не ошибкой.
        let ctx = NullExecutionContext;
        let out = UnicodeNormalize
            .apply(
                Cow::Borrowed("e\u{0301}".as_bytes()),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), "é".as_bytes());
    }
}
