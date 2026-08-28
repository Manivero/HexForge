//! `network.url_encode` / `network.url_decode` — percent-encoding по RFC 3986
//! (PRD FR §3.3 Сеть). Кодируются все байты кроме unreserved
//! (`A-Z a-z 0-9 - _ . ~`); декодирование принимает `%XX`, а также `+` как
//! пробел (application/x-www-form-urlencoded-совместимость).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~')
}

fn hex_hi(b: u8) -> u8 {
    match b >> 4 {
        0..=9 => b'0' + (b >> 4),
        _ => b'A' + (b >> 4) - 10,
    }
}

fn hex_lo(b: u8) -> u8 {
    match b & 0x0f {
        n @ 0..=9 => b'0' + n,
        _ => b'A' + (b & 0x0f) - 10,
    }
}

pub struct UrlEncode;

impl Transform for UrlEncode {
    fn id(&self) -> &'static str {
        "network.url_encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "URL Encode"
    }
    fn category(&self) -> &'static str {
        "Network"
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
        let mut out = Vec::with_capacity(input.len());
        for &b in input.as_ref() {
            if is_unreserved(b) {
                out.push(b);
            } else {
                out.push(b'%');
                out.push(hex_hi(b));
                out.push(hex_lo(b));
            }
        }
        Ok(Cow::Owned(out))
    }
}

pub struct UrlDecode;

impl Transform for UrlDecode {
    fn id(&self) -> &'static str {
        "network.url_decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "URL Decode"
    }
    fn category(&self) -> &'static str {
        "Network"
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
            match src[i] {
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                b'%' => {
                    if i + 2 >= src.len() {
                        return Err(TransformError::InvalidInput {
                            reason: format!("truncated % escape at byte {i}"),
                        });
                    }
                    let hi = (src[i + 1] as char).to_digit(16).ok_or_else(|| {
                        TransformError::InvalidInput {
                            reason: format!(
                                "invalid % escape at byte {i}: '{}{}'",
                                src[i + 1] as char,
                                src[i + 2] as char
                            ),
                        }
                    })? as u8;
                    let lo = (src[i + 2] as char).to_digit(16).ok_or_else(|| {
                        TransformError::InvalidInput {
                            reason: format!(
                                "invalid % escape at byte {i}: '{}{}'",
                                src[i + 1] as char,
                                src[i + 2] as char
                            ),
                        }
                    })? as u8;
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                other => {
                    out.push(other);
                    i += 1;
                }
            }
        }
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&UrlEncode) }
inventory::submit! { crate::TransformEntry(&UrlDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn encode_unreserved_pass_through() {
        let ctx = NullExecutionContext;
        let out = UrlEncode
            .apply(Cow::Borrowed(b"Az09-_.~"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"Az09-_.~");
    }

    #[test]
    fn encode_space_and_specials() {
        let ctx = NullExecutionContext;
        let out = UrlEncode
            .apply(Cow::Borrowed(b"Hello World!"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"Hello%20World%21");
    }

    #[test]
    fn decode_roundtrip_plus_semantics() {
        let ctx = NullExecutionContext;
        // Кодировщик не производит '+' (даёт %2B), но декодер принимает '+'
        // как пробел — form-urlencoded совместимость.
        let enc = UrlEncode
            .apply(Cow::Borrowed(b"a b+c%"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(enc.as_ref(), b"a%20b%2Bc%25");

        let dec = UrlDecode.apply(enc, &json!({}), &ctx).unwrap();
        assert_eq!(dec.as_ref(), b"a b+c%");
    }

    #[test]
    fn decode_truncated_escape_rejected() {
        let ctx = NullExecutionContext;
        for bad in [&b"%2"[..], &b"%"[..]] {
            let err = UrlDecode
                .apply(Cow::Borrowed(bad), &json!({}), &ctx)
                .unwrap_err();
            assert!(matches!(err, TransformError::InvalidInput { .. }));
        }
    }

    #[test]
    fn decode_invalid_hex_rejected() {
        let ctx = NullExecutionContext;
        let err = UrlDecode
            .apply(Cow::Borrowed(b"%zz"), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn utf8_multibyte_roundtrip() {
        let ctx = NullExecutionContext;
        let original = "привет";
        let enc = UrlEncode
            .apply(Cow::Borrowed(original.as_bytes()), &json!({}), &ctx)
            .unwrap();
        let dec = UrlDecode.apply(enc, &json!({}), &ctx).unwrap();
        assert_eq!(dec.as_ref(), original.as_bytes());
    }

    #[test]
    fn empty_input_roundtrip() {
        let ctx = NullExecutionContext;
        let enc = UrlEncode
            .apply(Cow::Borrowed(b""), &json!({}), &ctx)
            .unwrap();
        assert!(enc.is_empty());
        let dec = UrlDecode.apply(enc, &json!({}), &ctx).unwrap();
        assert!(dec.is_empty());
    }
}
