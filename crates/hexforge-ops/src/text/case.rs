//! `text.case_transform` — преобразование регистра ASCII-букв
//! (PRD §3.3 Text: "Case transforms"). Параметр `mode`: `upper`, `lower`,
//! `title`. Небуквенные байты проходят без изменений.

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct CaseTransform;

impl Transform for CaseTransform {
    fn id(&self) -> &'static str {
        "text.case_transform"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Case Transform"
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
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["mode"],
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["upper", "lower", "title"],
                    "default": "upper"
                }
            }
        })
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
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "mode".into(),
                reason: "string parameter 'mode' (upper|lower|title) is required".into(),
            })?;

        let out: Vec<u8> = match mode {
            "upper" => input.iter().map(|&b| b.to_ascii_uppercase()).collect(),
            "lower" => input.iter().map(|&b| b.to_ascii_lowercase()).collect(),
            "title" => {
                // Title case: первый буквенный символ каждого слова — uppercase.
                let mut prev_alpha = false;
                input
                    .iter()
                    .map(|&b| {
                        let is_alpha = b.is_ascii_alphabetic();
                        let result = if is_alpha && !prev_alpha {
                            b.to_ascii_uppercase()
                        } else if is_alpha {
                            b.to_ascii_lowercase()
                        } else {
                            b
                        };
                        prev_alpha = is_alpha;
                        result
                    })
                    .collect()
            }
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "mode".into(),
                    reason: format!("unknown mode '{mode}'; expected upper|lower|title"),
                })
            }
        };

        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&CaseTransform) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn upper_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(Cow::Borrowed(b"Hello World"), &serde_json::json!({"mode":"upper"}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"HELLO WORLD");
    }

    #[test]
    fn lower_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(Cow::Borrowed(b"Hello World"), &serde_json::json!({"mode":"lower"}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"hello world");
    }

    #[test]
    fn title_mode_capitalizes_word_starts() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"hello world foo"),
                &serde_json::json!({"mode":"title"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"Hello World Foo");
    }

    #[test]
    fn title_mode_lowercase_rest_of_word() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"HELLO WORLD"),
                &serde_json::json!({"mode":"title"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"Hello World");
    }

    #[test]
    fn non_letters_pass_through() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(Cow::Borrowed(b"a1.B_c"), &serde_json::json!({"mode":"upper"}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"A1.B_C");
    }

    #[test]
    fn unknown_mode_rejected() {
        let ctx = NullExecutionContext;
        let err = CaseTransform
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({"mode":"shout"}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn missing_mode_rejected() {
        let ctx = NullExecutionContext;
        let err = CaseTransform
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
