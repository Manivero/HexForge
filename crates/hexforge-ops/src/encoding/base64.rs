use base64::{engine::general_purpose, Engine as _};
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

fn engine_for(params: &serde_json::Value) -> &'static base64::engine::GeneralPurpose {
    match params.get("alphabet").and_then(|v| v.as_str()) {
        Some("url_safe") => &general_purpose::URL_SAFE,
        _ => &general_purpose::STANDARD,
    }
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
                    "enum": ["standard", "url_safe"],
                    "default": "standard"
                }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false, // MVP: base64 chunk-alignment (кратно 3 байтам) — потоковый путь запланирован post-MVP
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let encoded = engine_for(params).encode(input.as_ref());
        Ok(Cow::Owned(encoded.into_bytes()))
    }
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
                    "enum": ["standard", "url_safe"],
                    "default": "standard"
                }
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
        let decoded = engine_for(params)
            .decode(input.as_ref())
            .map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid base64: {e}"),
            })?;
        Ok(Cow::Owned(decoded))
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
}
