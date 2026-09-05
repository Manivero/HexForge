//! `text.html_encode` / `text.html_decode` — HTML entity encoding/decoding
//! (PRD §3.3 Text). Encode: `& < > " '` → named entities.
//! Decode: named (`&amp; &lt; &gt; &quot; &#39; &apos;`) + numeric
//! (`&#NN;` десятичный и `&#xHH;` шестнадцатеричный).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct HtmlEncode;

impl Transform for HtmlEncode {
    fn id(&self) -> &'static str {
        "text.html_encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "HTML Encode"
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
        // Байтовый проход: всё, кроме пяти спецсимволов, копируется как есть.
        // (Раньше здесь было `out.push(other as char)` на String — байты ≥0x80
        // превращались в char U+0080..U+00FF и перекодировались в UTF-8 заново,
        // молча портя любой не-ASCII ввод, напр. "héllo".)
        let mut out: Vec<u8> = Vec::with_capacity(input.len());
        for &b in input.as_ref() {
            match b {
                b'&' => out.extend_from_slice(b"&amp;"),
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#39;"),
                other => out.push(other),
            }
        }
        Ok(Cow::Owned(out))
    }
}

fn decode_named(name: &[u8]) -> Option<u8> {
    match name {
        b"amp" => Some(b'&'),
        b"lt" => Some(b'<'),
        b"gt" => Some(b'>'),
        b"quot" => Some(b'"'),
        b"#39" | b"apos" => Some(b'\''),
        _ => None,
    }
}

pub struct HtmlDecode;

impl Transform for HtmlDecode {
    fn id(&self) -> &'static str {
        "text.html_decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "HTML Decode"
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
        let src = input.as_ref();
        let mut out = Vec::with_capacity(src.len());
        let mut i = 0;
        while i < src.len() {
            if src[i] != b'&' {
                out.push(src[i]);
                i += 1;
                continue;
            }
            // Найти ';' в пределах разумной дистанции (макс 10 символов).
            let semi = src[i + 1..]
                .iter()
                .position(|&c| c == b';')
                .map(|p| i + 1 + p);
            let Some(semi_pos) = semi else {
                out.push(b'&');
                i += 1;
                continue;
            };
            if semi_pos - i > 11 {
                out.push(b'&');
                i += 1;
                continue;
            }

            let entity = &src[i + 1..semi_pos];
            if entity.first() == Some(&b'#') {
                // Числовая сущность &#NN; или &#xHH;
                let decoded = if entity.len() > 2 && (entity[1] == b'x' || entity[1] == b'X') {
                    u32::from_str_radix(&String::from_utf8_lossy(&entity[2..]), 16)
                        .ok()
                        .filter(|&v| v <= 0x10FFFF)
                        .and_then(char::from_u32)
                        .map(|ch| ch.to_string().into_bytes())
                } else {
                    String::from_utf8_lossy(&entity[1..])
                        .parse::<u32>()
                        .ok()
                        .filter(|&v| v <= 0x10FFFF)
                        .and_then(char::from_u32)
                        .map(|ch| ch.to_string().into_bytes())
                };
                match decoded {
                    Some(bytes) => {
                        out.extend_from_slice(&bytes);
                        i = semi_pos + 1;
                    }
                    None => {
                        out.extend_from_slice(b"&");
                        i += 1;
                    }
                }
            } else if let Some(ch) = decode_named(entity) {
                out.push(ch);
                i = semi_pos + 1;
            } else {
                // Неизвестная именованная сущность — оставить как есть.
                let span = semi_pos - i + 1;
                out.extend_from_slice(&src[i..i + span]);
                i = semi_pos + 1;
            }
        }
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&HtmlEncode) }
inventory::submit! { crate::TransformEntry(&HtmlDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn encode_named_entities() {
        let ctx = NullExecutionContext;
        let input = br#"<script>alert("xss")</script>&'"#;
        let expected = br#"&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;&amp;&#39;"#;
        let out = HtmlEncode
            .apply(Cow::Borrowed(input), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), &expected[..]);
    }

    #[test]
    fn encode_plain_text_passthrough() {
        let ctx = NullExecutionContext;
        let out = HtmlEncode
            .apply(Cow::Borrowed(b"Hello World"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"Hello World");
    }

    #[test]
    fn encode_preserves_non_ascii_bytes() {
        // "héllo © <b>" в UTF-8: escape только ASCII-спецсимволы,
        // многобайтовые последовательности копируются байт-в-байт.
        let ctx = NullExecutionContext;
        let input = "héllo © <b>".as_bytes();
        let out = HtmlEncode
            .apply(Cow::Borrowed(input), &json!({}), &ctx)
            .unwrap();
        let expected = "héllo © &lt;b&gt;".as_bytes();
        assert_eq!(out.as_ref(), expected);
    }

    #[test]
    fn roundtrip_preserves_non_ascii() {
        let ctx = NullExecutionContext;
        let original = "Tom & Jerry héllo ©";
        let enc = HtmlEncode
            .apply(Cow::Borrowed(original.as_bytes()), &json!({}), &ctx)
            .unwrap();
        let dec = HtmlDecode.apply(enc, &json!({}), &ctx).unwrap();
        assert_eq!(dec.as_ref(), original.as_bytes());
    }

    #[test]
    fn decode_roundtrip() {
        let ctx = NullExecutionContext;
        let original = r#"<b class="x">Tom & Jerry's</b>"#;
        let enc = HtmlEncode
            .apply(Cow::Borrowed(original.as_bytes()), &json!({}), &ctx)
            .unwrap();
        let dec = HtmlDecode.apply(enc, &json!({}), &ctx).unwrap();
        assert_eq!(dec.as_ref(), original.as_bytes());
    }

    #[test]
    fn decode_numeric_entities() {
        let ctx = NullExecutionContext;
        let dec = HtmlDecode
            .apply(Cow::Borrowed(b"&#72;&#101;llo &#x48;i"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"Hello Hi");
    }

    #[test]
    fn decode_unknown_entity_left_as_is() {
        let ctx = NullExecutionContext;
        let dec = HtmlDecode
            .apply(Cow::Borrowed(b"&nosuch; ok"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"&nosuch; ok");
    }

    #[test]
    fn decode_stray_ampersand_preserved() {
        let ctx = NullExecutionContext;
        let dec = HtmlDecode
            .apply(Cow::Borrowed(b"a & b"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"a & b");
    }

    #[test]
    fn empty_input() {
        let ctx = NullExecutionContext;
        let e = HtmlEncode
            .apply(Cow::Borrowed(b""), &json!({}), &ctx)
            .unwrap();
        assert!(e.is_empty());
        let d = HtmlDecode.apply(e, &json!({}), &ctx).unwrap();
        assert!(d.is_empty());
    }
}
