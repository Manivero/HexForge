//! `crypto.xor` — побайтовый XOR с циклическим UTF-8 ключом
//! (PRD FR §3.3 Cryptography: "XOR single/multi-byte key").

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct XorCipher;

fn key_bytes(params: &serde_json::Value) -> Result<Vec<u8>, TransformError> {
    let key = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
        TransformError::InvalidParameter {
            field: "key".into(),
            reason: "string parameter 'key' is required (UTF-8, cycled over input)".into(),
        }
    })?;
    let bytes = key.as_bytes().to_vec();
    if bytes.is_empty() {
        return Err(TransformError::InvalidParameter {
            field: "key".into(),
            reason: "key must not be empty".into(),
        });
    }
    Ok(bytes)
}

impl Transform for XorCipher {
    fn id(&self) -> &'static str {
        "crypto.xor"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "XOR"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false, // MVP: полный буфер; чанкование требует позиции в ключе
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": { "type": "string", "description": "UTF-8 key, cycled over input" }
            }
        })
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let key = key_bytes(params)?;
        // Пустой вход с непустым ключом — корректный пустой выход.
        let out: Vec<u8> = input
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % key.len()])
            .collect();
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&XorCipher) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn single_byte_key_known_vector() {
        // Ключ U+0010 — однобайтный в UTF-8.
        let ctx = NullExecutionContext;
        let out = XorCipher
            .apply(
                Cow::Borrowed(&[0x00, 0x01, 0x02]),
                &serde_json::json!({ "key": "\u{10}" }),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), &[0x10, 0x11, 0x12]);
    }

    #[test]
    fn multi_byte_key_cycles() {
        let ctx = NullExecutionContext;
        // Ключ [1,2] по трём байтам: b^1, b^2, b^1
        let out = XorCipher
            .apply(
                Cow::Borrowed(&[0x00, 0x00, 0x00]),
                &serde_json::json!({ "key": "\u{1}\u{2}" }),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), &[0x01, 0x02, 0x01]);
    }

    #[test]
    fn xor_is_involution() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({ "key": "секрет" }); // UTF-8 мультибайт
        let once = XorCipher
            .apply(Cow::Borrowed(b"Hello"), &params, &ctx)
            .unwrap();
        assert_ne!(once.as_ref(), b"Hello");
        let twice = XorCipher.apply(once, &params, &ctx).unwrap();
        assert_eq!(twice.as_ref(), b"Hello");
    }

    #[test]
    fn empty_key_rejected() {
        let ctx = NullExecutionContext;
        let err = XorCipher
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({ "key": "" }), &ctx)
            .unwrap_err();
        assert!(matches!(
            err,
            TransformError::InvalidParameter { ref field, .. } if field == "key"
        ));
    }

    #[test]
    fn missing_key_rejected() {
        let ctx = NullExecutionContext;
        let err = XorCipher
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn empty_input_empty_output() {
        let ctx = NullExecutionContext;
        let out = XorCipher
            .apply(Cow::Borrowed(b""), &serde_json::json!({ "key": "k" }), &ctx)
            .unwrap();
        assert!(out.is_empty());
    }
}
