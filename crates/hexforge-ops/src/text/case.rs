//! `text.case_transform` — преобразование регистра ASCII-букв
//! (PRD §3.3 Text: "Case transforms"). Параметр `mode`: `upper`, `lower`,
//! `title`. Небуквенные байты проходят без изменений.

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev_is_lower = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            let is_upper = c.is_ascii_uppercase();
            let is_lower = c.is_ascii_lowercase();
            if is_upper && prev_is_lower && !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c.to_ascii_lowercase());
            prev_is_lower = is_lower;
        } else {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            prev_is_lower = false;
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().filter(|w| !w.is_empty()).collect()
}

fn to_snake_case(s: &str) -> String {
    split_words(s).join("_")
}
fn to_kebab_case(s: &str) -> String {
    split_words(s).join("-")
}
fn to_camel_case(s: &str) -> String {
    let words = split_words(s);
    let mut out = String::new();
    for (i, w) in words.into_iter().enumerate() {
        if i == 0 {
            out.push_str(&w);
        } else {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}
fn to_pascal_case(s: &str) -> String {
    split_words(s)
        .into_iter()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

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
                    "enum": ["upper", "lower", "title", "snake", "kebab", "camel", "pascal"],
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
        let mode = params.get("mode").and_then(|v| v.as_str()).ok_or_else(|| {
            TransformError::InvalidParameter {
                field: "mode".into(),
                reason: "string parameter 'mode' (upper|lower|title) is required".into(),
            }
        })?;

        let out: Vec<u8> = match mode {
            "upper" => input.iter().map(|&b| b.to_ascii_uppercase()).collect(),
            "lower" => input.iter().map(|&b| b.to_ascii_lowercase()).collect(),
            "title" => {
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
            "snake" => to_snake_case(&String::from_utf8_lossy(input.as_ref())).into_bytes(),
            "kebab" => to_kebab_case(&String::from_utf8_lossy(input.as_ref())).into_bytes(),
            "camel" => to_camel_case(&String::from_utf8_lossy(input.as_ref())).into_bytes(),
            "pascal" => to_pascal_case(&String::from_utf8_lossy(input.as_ref())).into_bytes(),
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "mode".into(),
                    reason: format!(
                        "unknown mode '{mode}'; expected upper|lower|title|snake|kebab|camel|pascal"
                    ),
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
            .apply(
                Cow::Borrowed(b"Hello World"),
                &serde_json::json!({"mode":"upper"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"HELLO WORLD");
    }

    #[test]
    fn lower_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"Hello World"),
                &serde_json::json!({"mode":"lower"}),
                &ctx,
            )
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
            .apply(
                Cow::Borrowed(b"a1.B_c"),
                &serde_json::json!({"mode":"upper"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"A1.B_C");
    }

    #[test]
    fn unknown_mode_rejected() {
        let ctx = NullExecutionContext;
        let err = CaseTransform
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"mode":"shout"}),
                &ctx,
            )
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

    #[test]
    fn snake_mode() {
        let ctx = NullExecutionContext;
        let cases = [
            ("Hello World", "hello_world"),
            ("hello-world", "hello_world"),
            ("helloWorld", "hello_world"),
            ("HelloWorld", "hello_world"),
            ("  hello   world  ", "hello_world"),
            ("", ""),
        ];
        for (input, expected) in cases {
            let out = CaseTransform
                .apply(
                    Cow::Borrowed(input.as_bytes()),
                    &serde_json::json!({"mode":"snake"}),
                    &ctx,
                )
                .unwrap();
            assert_eq!(out.as_ref(), expected.as_bytes(), "input {input:?}");
        }
    }

    #[test]
    fn kebab_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"Hello World Test"),
                &serde_json::json!({"mode":"kebab"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"hello-world-test");
    }

    #[test]
    fn camel_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"hello world test"),
                &serde_json::json!({"mode":"camel"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"helloWorldTest");
        let out2 = CaseTransform
            .apply(
                Cow::Borrowed(b"Hello-world_test"),
                &serde_json::json!({"mode":"camel"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out2.as_ref(), b"helloWorldTest");
    }

    #[test]
    fn pascal_mode() {
        let ctx = NullExecutionContext;
        let out = CaseTransform
            .apply(
                Cow::Borrowed(b"hello world"),
                &serde_json::json!({"mode":"pascal"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"HelloWorld");
        let out2 = CaseTransform
            .apply(
                Cow::Borrowed(b"hello-world_test"),
                &serde_json::json!({"mode":"pascal"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out2.as_ref(), b"HelloWorldTest");
    }
}
