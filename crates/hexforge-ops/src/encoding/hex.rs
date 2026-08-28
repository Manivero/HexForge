use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct HexEncode;

impl Transform for HexEncode {
    fn id(&self) -> &'static str {
        "encoding.hex.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "To Hex"
    }
    fn category(&self) -> &'static str {
        "Encoding"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: true, // каждый входной байт независимо мапится на 2 hex-символа — тривиально потоково
            memory_cost: MemoryCost::PerChunk,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        Ok(Cow::Owned(hex::encode(input.as_ref()).into_bytes()))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        _is_last: bool,
        _state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        // Каждый байт мапится независимо на два hex-символа: состояние не нужно.
        Ok(hex::encode(chunk).into_bytes())
    }
}

pub struct HexDecode;

/// Per-node состояние chunked-декодирования: незакрытый старший ниббл на
/// границе чанков (после фильтрации whitespace пары байтов могут разрываться
/// между чанками — "4" + "86a" должно декодироваться как единое "486a").
#[derive(Default)]
struct HexDecodeState {
    pending_high: Option<u8>,
}

impl Transform for HexDecode {
    fn id(&self) -> &'static str {
        "encoding.hex.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "From Hex"
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
        // Пробелы/переносы строк — обычный шум в hex-дампах, игнорируем их.
        let cleaned: String = input
            .iter()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|&b| b as char)
            .collect();
        let decoded = hex::decode(&cleaned).map_err(|e| TransformError::InvalidInput {
            reason: format!("not valid hex: {e}"),
        })?;
        Ok(Cow::Owned(decoded))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        // Состояние принадлежит планировщику; первый чанк получает пустой
        // Box(()) — операция засеивает свой конкретный тип (инвариант трейта).
        if state.downcast_ref::<HexDecodeState>().is_none() {
            *state = Box::new(HexDecodeState::default());
        }
        let st = state
            .downcast_mut::<HexDecodeState>()
            .expect("HexDecodeState seeded above");

        let mut out = Vec::with_capacity(chunk.len() / 2);
        for &b in chunk {
            if b.is_ascii_whitespace() {
                continue;
            }
            let nibble = (b as char)
                .to_digit(16)
                .ok_or_else(|| TransformError::InvalidInput {
                    reason: format!("not valid hex: invalid character '{}'", b as char),
                })? as u8;
            match st.pending_high.take() {
                None => st.pending_high = Some(nibble),
                Some(high) => out.push(high * 16 + nibble),
            }
        }
        if is_last && st.pending_high.is_some() {
            return Err(TransformError::InvalidInput {
                reason: "not valid hex: odd number of digits".into(),
            });
        }
        Ok(out)
    }
}

inventory::submit! { crate::TransformEntry(&HexEncode) }
inventory::submit! { crate::TransformEntry(&HexDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn roundtrip() {
        let input: ByteView = Cow::Borrowed(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});

        let encoded = HexEncode.apply(input.clone(), &params, &ctx).unwrap();
        assert_eq!(encoded.as_ref(), b"deadbeef");

        let decoded = HexDecode.apply(encoded, &params, &ctx).unwrap();
        assert_eq!(decoded.as_ref(), input.as_ref());
    }

    #[test]
    fn decode_ignores_whitespace_noise() {
        let input: ByteView = Cow::Borrowed(b"de ad be ef\n");
        let ctx = NullExecutionContext;
        let decoded = HexDecode
            .apply(input, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(decoded.as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn chunked_decode_pairs_nibbles_across_boundaries() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());

        let first = HexDecode
            .apply_chunk(b"4 8", false, &mut state, &params, &ctx)
            .unwrap();
        assert_eq!(first, vec![0x48]);

        // Ниббл "6" остаётся незакрытым до следующего чанка.
        let second = HexDecode
            .apply_chunk(b"6", false, &mut state, &params, &ctx)
            .unwrap();
        assert!(second.is_empty());

        // Разрыв пары внутри слова + whitespace-шум на границе.
        let third = HexDecode
            .apply_chunk(b"\na", true, &mut state, &params, &ctx)
            .unwrap();
        assert_eq!(third, vec![0x6A]);
    }

    #[test]
    fn chunked_decode_rejects_odd_total_length() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());

        HexDecode
            .apply_chunk(b"4", false, &mut state, &params, &ctx)
            .unwrap();
        let err = HexDecode
            .apply_chunk(b"", true, &mut state, &params, &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn chunked_encode_matches_apply_on_split_input() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});

        let whole = HexEncode
            .apply(Cow::Borrowed(b"deadbeef"), &params, &ctx)
            .unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        for (i, part) in [b"de".as_slice(), b"ad", b"be", b"ef"].iter().enumerate() {
            chunked.extend_from_slice(
                &HexEncode
                    .apply_chunk(part, i == 3, &mut state, &params, &ctx)
                    .unwrap(),
            );
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }
}
