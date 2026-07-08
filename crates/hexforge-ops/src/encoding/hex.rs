use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
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
}

pub struct HexDecode;

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
        let decoded = HexDecode.apply(input, &serde_json::json!({}), &ctx).unwrap();
        assert_eq!(decoded.as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
