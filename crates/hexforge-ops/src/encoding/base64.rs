use base64::{
    alphabet::Alphabet,
    engine::{general_purpose, GeneralPurpose},
    Engine as _,
};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

fn engine_for(params: &serde_json::Value) -> Result<Cow<'static, GeneralPurpose>, TransformError> {
    match params.get("alphabet").and_then(|v| v.as_str()) {
        Some("url_safe") => Ok(Cow::Borrowed(&general_purpose::URL_SAFE)),
        Some("custom") => {
            let custom = params
                .get("custom_alphabet")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TransformError::InvalidParameter {
                    field: "custom_alphabet".into(),
                    reason: "custom alphabet required when alphabet='custom' (64-char string)"
                        .into(),
                })?;
            if custom.len() != 64 {
                return Err(TransformError::InvalidParameter {
                    field: "custom_alphabet".into(),
                    reason: format!("custom alphabet must be 64 chars, got {}", custom.len()),
                });
            }
            let alphabet = Alphabet::new(custom).map_err(|e| TransformError::InvalidParameter {
                field: "custom_alphabet".into(),
                reason: format!("invalid alphabet: {e}"),
            })?;
            Ok(Cow::Owned(GeneralPurpose::new(
                &alphabet,
                general_purpose::PAD,
            )))
        }
        _ => Ok(Cow::Borrowed(&general_purpose::STANDARD)),
    }
}

#[derive(Default)]
struct Base64EncodeState {
    leftover: Vec<u8>, // 0..2 bytes pending from previous chunk
}

pub struct Base64Encode;

impl Transform for Base64Encode {
    fn id(&self) -> &'static str {
        "encoding.base64.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base64 Encode"
    }
    fn category(&self) -> &'static str {
        "Encoding"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "alphabet": {
                    "type": "string",
                    "enum": ["standard", "url_safe", "custom"],
                    "default": "standard"
                },
                "custom_alphabet": {
                    "type": "string",
                    "description": "64-char custom alphabet for alphabet='custom'"
                }
            }
        })
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
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let engine = engine_for(params)?;
        let encoded = engine.encode(input.as_ref());
        Ok(Cow::Owned(encoded.into_bytes()))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Base64EncodeState>().is_none() {
            *state = Box::new(Base64EncodeState::default());
        }
        let st = state
            .downcast_mut::<Base64EncodeState>()
            .expect("Base64EncodeState seeded");
        let engine = engine_for(params)?;

        // Combine leftover from previous chunk with new chunk
        let mut combined = Vec::with_capacity(st.leftover.len() + chunk.len());
        combined.extend_from_slice(&st.leftover);
        combined.extend_from_slice(chunk);

        let complete_len = (combined.len() / 3) * 3;
        let leftover_len = combined.len() % 3;

        let to_encode = &combined[..complete_len];
        let mut out = if to_encode.is_empty() {
            Vec::new()
        } else {
            engine.encode(to_encode).into_bytes()
        };

        if is_last {
            if leftover_len > 0 {
                let tail = &combined[complete_len..];
                out.extend_from_slice(engine.encode(tail).as_bytes());
            }
            st.leftover.clear();
        } else {
            st.leftover.clear();
            if leftover_len > 0 {
                st.leftover.extend_from_slice(&combined[complete_len..]);
            }
        }
        Ok(out)
    }
}

#[derive(Default)]
struct Base64DecodeState {
    leftover: String, // 0..3 base64 chars pending (without padding, whitespace stripped)
}

pub struct Base64Decode;

impl Transform for Base64Decode {
    fn id(&self) -> &'static str {
        "encoding.base64.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base64 Decode"
    }
    fn category(&self) -> &'static str {
        "Encoding"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "alphabet": {
                    "type": "string",
                    "enum": ["standard", "url_safe", "custom"],
                    "default": "standard"
                },
                "custom_alphabet": {
                    "type": "string",
                    "description": "64-char custom alphabet for alphabet='custom'"
                }
            }
        })
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
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let engine = engine_for(params)?;
        // For non-streaming path, we allow whitespace by filtering before decode.
        // Engine with PAD requires correct padding; we filter whitespace and try decode.
        let filtered: String = input
            .as_ref()
            .iter()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|&b| b as char)
            .collect();
        let decoded = engine
            .decode(filtered.trim())
            .map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid base64: {e}"),
            })?;
        Ok(Cow::Owned(decoded))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Base64DecodeState>().is_none() {
            *state = Box::new(Base64DecodeState::default());
        }
        let st = state
            .downcast_mut::<Base64DecodeState>()
            .expect("Base64DecodeState seeded");
        let engine = engine_for(params)?;

        // Filter whitespace from chunk, keep base64 chars and padding
        let filtered: String = chunk
            .iter()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|&b| b as char)
            .collect();

        // Append to leftover
        st.leftover.push_str(&filtered);

        // If padding appears, it must be at the very end and only on last chunk
        if !is_last && st.leftover.contains('=') {
            return Err(TransformError::InvalidInput {
                reason: "not valid base64: padding '=' found before final chunk".into(),
            });
        }

        // Process complete 4-char groups
        let mut out = Vec::new();
        while st.leftover.len() >= 4 {
            // Peek at first 4 chars — if they contain padding, handle at is_last
            let four = st.leftover[..4].to_string();
            if four.contains('=') && !is_last {
                break; // Wait for final chunk to handle padding
            }
            // If leftover has >=4 and we are at is_last, we will handle padding after loop
            // For now, decode complete groups without padding
            let to_decode = &st.leftover[..4];
            // If to_decode contains padding, ensure it's only at the end and is_last
            if to_decode.contains('=') {
                break;
            }
            let decoded = engine
                .decode(to_decode)
                .map_err(|e| TransformError::InvalidInput {
                    reason: format!("not valid base64: {e}"),
                })?;
            out.extend_from_slice(&decoded);
            st.leftover.drain(..4);
        }

        if is_last {
            if st.leftover.is_empty() {
                return Ok(out);
            }
            // Validate leftover length: 1 is invalid (needs 4), 2 or 3 with padding is valid
            if st.leftover.len() == 1 {
                return Err(TransformError::InvalidInput {
                    reason: "not valid base64: leftover 1 char (invalid length)".into(),
                });
            }
            // Leftover may contain padding already, let engine handle it
            // Ensure we have correct padding: engine requires correct padding, but we can pad if missing
            let to_decode = st.leftover.clone();
            // If no padding and length 2 or 3, pad to 4 for engine with PAD? Engine will error if not padded correctly.
            // We try direct decode first; if fails due to padding, try adding padding.
            let decoded = match engine.decode(&to_decode) {
                Ok(v) => v,
                Err(_) => {
                    // Try adding padding
                    let mut padded = to_decode.clone();
                    while padded.len() % 4 != 0 {
                        padded.push('=');
                    }
                    engine
                        .decode(&padded)
                        .map_err(|e| TransformError::InvalidInput {
                            reason: format!("not valid base64: {e}"),
                        })?
                }
            };
            out.extend_from_slice(&decoded);
            st.leftover.clear();
        }

        Ok(out)
    }
}

inventory::submit! { crate::TransformEntry(&Base64Encode) }
inventory::submit! { crate::TransformEntry(&Base64Decode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn roundtrip_standard() {
        let input: ByteView = Cow::Borrowed(b"HexForge > CyberChef");
        let params = serde_json::json!({ "alphabet": "standard" });
        let ctx = NullExecutionContext;

        let encoded = Base64Encode.apply(input.clone(), &params, &ctx).unwrap();
        let decoded = Base64Decode.apply(encoded, &params, &ctx).unwrap();
        assert_eq!(decoded.as_ref(), input.as_ref());
    }

    #[test]
    fn decode_rejects_invalid_input() {
        let input: ByteView = Cow::Borrowed(b"not-valid-base64!!!");
        let params = serde_json::json!({});
        let ctx = NullExecutionContext;
        let result = Base64Decode.apply(input, &params, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn custom_alphabet_roundtrip() {
        let custom = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let input: ByteView = Cow::Borrowed(b"Hello custom alphabet");
        let params = serde_json::json!({ "alphabet": "custom", "custom_alphabet": custom });
        let ctx = NullExecutionContext;
        let encoded = Base64Encode.apply(input.clone(), &params, &ctx).unwrap();
        let decoded = Base64Decode.apply(encoded, &params, &ctx).unwrap();
        assert_eq!(decoded.as_ref(), input.as_ref());
    }

    #[test]
    fn custom_alphabet_rejects_invalid_length() {
        let input: ByteView = Cow::Borrowed(b"x");
        let params = serde_json::json!({ "alphabet": "custom", "custom_alphabet": "short" });
        let ctx = NullExecutionContext;
        let err = Base64Encode.apply(input, &params, &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn chunked_encode_matches_apply() {
        let input = b"Hello HexForge streaming base64 test with 64MiB chunks!";
        let params = serde_json::json!({ "alphabet": "standard" });
        let ctx = NullExecutionContext;
        let whole = Base64Encode
            .apply(Cow::Borrowed(input), &params, &ctx)
            .unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        let chunk_size = 5; // intentionally not multiple of 3
        let mut offset = 0;
        while offset < input.len() {
            let end = (offset + chunk_size).min(input.len());
            let is_last = end == input.len();
            chunked.extend_from_slice(
                &Base64Encode
                    .apply_chunk(&input[offset..end], is_last, &mut state, &params, &ctx)
                    .unwrap(),
            );
            offset = end;
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }

    #[test]
    fn chunked_decode_matches_apply() {
        let params = serde_json::json!({ "alphabet": "standard" });
        let ctx = NullExecutionContext;
        let original = b"Hello streaming decode!";
        let encoded = Base64Encode
            .apply(Cow::Borrowed(original), &params, &ctx)
            .unwrap();

        let whole = Base64Decode.apply(encoded.clone(), &params, &ctx).unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        let chunk_size = 7; // not multiple of 4
        let mut offset = 0;
        while offset < encoded.len() {
            let end = (offset + chunk_size).min(encoded.len());
            let is_last = end == encoded.len();
            chunked.extend_from_slice(
                &Base64Decode
                    .apply_chunk(&encoded[offset..end], is_last, &mut state, &params, &ctx)
                    .unwrap(),
            );
            offset = end;
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }

    #[test]
    fn chunked_decode_rejects_invalid_padding_midstream() {
        let params = serde_json::json!({});
        let ctx = NullExecutionContext;
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let err = Base64Decode
            .apply_chunk(b"SGVs", false, &mut state, &params, &ctx)
            .unwrap();
        assert_eq!(err, b"Hel".to_vec());
        // Padding before final chunk should be rejected
        let err = Base64Decode.apply_chunk(b"bG8=", false, &mut state, &params, &ctx);
        assert!(err.is_err());
    }

    #[test]
    fn chunked_roundtrip_random_split() {
        let params = serde_json::json!({ "alphabet": "standard" });
        let ctx = NullExecutionContext;
        let data: Vec<u8> = (0..255).collect();
        let encoded = Base64Encode
            .apply(Cow::Borrowed(&data), &params, &ctx)
            .unwrap();
        let mut state_enc: Box<dyn std::any::Any + Send> = Box::new(());
        let mut state_dec: Box<dyn std::any::Any + Send> = Box::new(());
        // Encode in random 1..10 byte chunks, decode in random 1..8 char chunks
        let mut chunked_enc = Vec::new();
        let mut off = 0;
        while off < data.len() {
            let sz = (off % 7 + 1).min(data.len() - off);
            let is_last = off + sz == data.len();
            chunked_enc.extend_from_slice(
                &Base64Encode
                    .apply_chunk(&data[off..off + sz], is_last, &mut state_enc, &params, &ctx)
                    .unwrap(),
            );
            off += sz;
        }
        assert_eq!(encoded.as_ref(), chunked_enc.as_slice());
        // Now decode chunked_enc via chunked decode
        let mut decoded = Vec::new();
        let mut off2 = 0;
        while off2 < chunked_enc.len() {
            let sz = (off2 % 5 + 1).min(chunked_enc.len() - off2);
            let is_last = off2 + sz == chunked_enc.len();
            decoded.extend_from_slice(
                &Base64Decode
                    .apply_chunk(
                        &chunked_enc[off2..off2 + sz],
                        is_last,
                        &mut state_dec,
                        &params,
                        &ctx,
                    )
                    .unwrap(),
            );
            off2 += sz;
        }
        assert_eq!(decoded, data);
    }
}
