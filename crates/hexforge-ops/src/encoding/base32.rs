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

#[derive(Default)]
struct Base32EncodeState {
    leftover: Vec<u8>, // 0..4 bytes
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
            streamable: true,
            memory_cost: MemoryCost::PerChunk,
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

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Base32EncodeState>().is_none() {
            *state = Box::new(Base32EncodeState::default());
        }
        let st = state.downcast_mut::<Base32EncodeState>().expect("Base32EncodeState seeded");

        let mut combined = Vec::with_capacity(st.leftover.len() + chunk.len());
        combined.extend_from_slice(&st.leftover);
        combined.extend_from_slice(chunk);

        let complete_len = (combined.len() / 5) * 5;
        let mut out = String::with_capacity(complete_len / 5 * 8);

        let mut bits: u32 = 0;
        let mut count: u32 = 0;
        for &b in &combined[..complete_len] {
            bits = (bits << 8) | b as u32;
            count += 8;
            while count >= 5 {
                count -= 5;
                out.push(ALPHABET[((bits >> count) & 0x1f) as usize] as char);
            }
        }
        // At this point count should be 0 because complete_len is multiple of 5 and we consumed all bits
        // But we still need to handle leftover for next chunk or final padding
        if is_last {
            let tail = &combined[complete_len..];
            if !tail.is_empty() {
                let mut bits2: u32 = 0;
                let mut count2: u32 = 0;
                for &b in tail {
                    bits2 = (bits2 << 8) | b as u32;
                    count2 += 8;
                    while count2 >= 5 {
                        count2 -= 5;
                        out.push(ALPHABET[((bits2 >> count2) & 0x1f) as usize] as char);
                    }
                }
                if count2 > 0 {
                    out.push(ALPHABET[((bits2 << (5 - count2)) & 0x1f) as usize] as char);
                }
                while !out.len().is_multiple_of(8) {
                    out.push('=');
                }
            }
            st.leftover.clear();
        } else {
            st.leftover.clear();
            st.leftover.extend_from_slice(&combined[complete_len..]);
        }
        Ok(out.into_bytes())
    }
}

#[derive(Default)]
struct Base32DecodeState {
    bits: u32,
    count: u32,
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
            streamable: true,
            memory_cost: MemoryCost::PerChunk,
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

        if count > 0 && (bits & ((1 << count) - 1)) != 0 {
            return Err(TransformError::InvalidInput {
                reason: "non-zero trailing bits in base32 input".into(),
            });
        }

        Ok(Cow::Owned(out))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Base32DecodeState>().is_none() {
            *state = Box::new(Base32DecodeState::default());
        }
        let st = state.downcast_mut::<Base32DecodeState>().expect("Base32DecodeState seeded");

        let mut out = Vec::new();
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' || b.is_ascii_whitespace() {
                continue;
            }
            let v = value_of(b).ok_or_else(|| TransformError::InvalidInput {
                reason: format!(
                    "invalid base32 character '{}' at position {i}",
                    b as char
                ),
            })?;
            st.bits = (st.bits << 5) | v as u32;
            st.count += 5;
            if st.count >= 8 {
                st.count -= 8;
                out.push((st.bits >> st.count) as u8);
            }
        }

        if is_last {
            if st.count > 0 && (st.bits & ((1 << st.count) - 1)) != 0 {
                return Err(TransformError::InvalidInput {
                    reason: "non-zero trailing bits in base32 input".into(),
                });
            }
            // Clear state for next run (though run is over)
            st.bits = 0;
            st.count = 0;
        }

        Ok(out)
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
        let err = Base32Decode
            .apply(Cow::Borrowed(b"AC"), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn chunked_encode_matches_apply() {
        let ctx = NullExecutionContext;
        let params = json!({});
        let data = b"Hello Base32 streaming test with 5-byte groups!";
        let whole = Base32Encode.apply(Cow::Borrowed(data), &params, &ctx).unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        let chunk_size = 7; // not multiple of 5
        let mut off = 0;
        while off < data.len() {
            let end = (off + chunk_size).min(data.len());
            let is_last = end == data.len();
            chunked.extend_from_slice(
                &Base32Encode
                    .apply_chunk(&data[off..end], is_last, &mut state, &params, &ctx)
                    .unwrap(),
            );
            off = end;
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }

    #[test]
    fn chunked_decode_matches_apply() {
        let ctx = NullExecutionContext;
        let params = json!({});
        let data: Vec<u8> = (0..20).collect();
        let enc = Base32Encode.apply(Cow::Borrowed(&data), &params, &ctx).unwrap();

        let whole = Base32Decode.apply(enc.clone(), &params, &ctx).unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        let chunk_size = 6; // not multiple of 8
        let mut off = 0;
        while off < enc.len() {
            let end = (off + chunk_size).min(enc.len());
            let is_last = end == enc.len();
            chunked.extend_from_slice(
                &Base32Decode
                    .apply_chunk(&enc[off..end], is_last, &mut state, &params, &ctx)
                    .unwrap(),
            );
            off = end;
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }
}
