use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn value_of(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a'),
        b'2'..=b'7' => Some(b - b'2' + 26),
        _ => None,
    }
}

pub struct Base32Encode;

impl Transform for Base32Encode {
    fn id(&self) -> &'static str {
        "encoding.base32.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base32 Encode"
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
        let data = input.as_ref();
        let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
        let mut bits: u32 = 0;
        let mut count: u32 = 0;
        for &b in data {
            bits = (bits << 8) | b as u32;
            count += 8;
            while count >= 5 {
                count -= 5;
                out.push(ALPHABET[((bits >> count) & 0x1f) as usize] as char);
            }
        }
        if count > 0 {
            out.push(ALPHABET[((bits << (5 - count)) & 0x1f) as usize] as char);
        }
        while !out.len().is_multiple_of(8) {
            out.push('=');
        }
        Ok(Cow::Owned(out.into_bytes()))
    }
}

pub struct Base32Decode;

impl Transform for Base32Decode {
    fn id(&self) -> &'static str {
        "encoding.base32.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base32 Decode"
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
        let mut bits: u32 = 0;
        let mut count: u32 = 0;
        let mut out: Vec<u8> = Vec::new();

        for (i, &b) in input.as_ref().iter().enumerate() {
            if b == b'=' || b.is_ascii_whitespace() {
                continue;
            }
            let v = value_of(b).ok_or_else(|| TransformError::InvalidInput {
                reason: format!(
                    "invalid base32 character '{}' at position {i}",
                    b as char
                ),
            })?;
            bits = (bits << 5) | v as u32;
            count += 5;
            if count >= 8 {
                count -= 8;
                out.push((bits >> count) as u8);
            }
        }

        // Оставшиеся биты (< 8) должны быть нулями — иначе усечённые данные.
        if count > 0 && (bits & ((1 << count) - 1)) != 0 {
            return Err(TransformError::InvalidInput {
                reason: "non-zero trailing bits in base32 input".into(),
            });
        }

        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&Base32Encode) }
inventory::submit! { crate::TransformEntry(&Base32Decode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn rfc4648_test_vectors() {
        let ctx = NullExecutionContext;
        for (input, expected) in [
            (&b""[..], ""),
            ("f".as_bytes(), "MY======"),
            ("fo".as_bytes(), "MZXQ===="),
            ("foo".as_bytes(), "MZXW6==="),
            ("foob".as_bytes(), "MZXW6YQ="),
            ("fooba".as_bytes(), "MZXW6YTB"),
            ("foobar".as_bytes(), "MZXW6YTBOI======"),
        ] {
            let out = Base32Encode
                .apply(Cow::Borrowed(input), &json!({}), &ctx)
                .unwrap();
            assert_eq!(out.as_ref(), expected.as_bytes(), "input: {input:?}");
        }
    }

    #[test]
    fn decode_roundtrip_all_lengths() {
        let ctx = NullExecutionContext;
        for len in 0usize..=11 {
            let data: Vec<u8> = (0..len as u8).map(|b| b.wrapping_mul(37)).collect();
            let enc = Base32Encode
                .apply(Cow::Borrowed(&data), &json!({}), &ctx)
                .unwrap();
            let dec = Base32Decode.apply(enc, &json!({}), &ctx).unwrap();
            assert_eq!(dec.as_ref(), data.as_slice(), "len={len}");
        }
    }

    #[test]
    fn decode_case_insensitive_and_whitespace() {
        let ctx = NullExecutionContext;
        let dec = Base32Decode
            .apply(Cow::Borrowed(b"mzxw6 yt b"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"fooba");
    }

    #[test]
    fn decode_rejects_non_alphabet() {
        let ctx = NullExecutionContext;
        let err = Base32Decode
            .apply(Cow::Borrowed(b"MZX109"), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn decode_rejects_nonzero_trailing_bits() {
        let ctx = NullExecutionContext;
        // 'A' = 00000, 'C' = 00010 — последний символ оставляет ненулевые биты.
        let err = Base32Decode
            .apply(Cow::Borrowed(b"AC"), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
