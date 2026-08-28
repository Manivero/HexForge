//! `crypto.chacha20_poly1305` — ChaCha20-Poly1305 AEAD (RFC 8439).
//! Uses `chacha20poly1305` crate with 256-bit key, 96-bit nonce, 128-bit auth tag.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct ChaCha20Poly1305Encrypt;

impl Transform for ChaCha20Poly1305Encrypt {
    fn id(&self) -> &'static str {
        "crypto.chacha20_poly1305.encrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "ChaCha20-Poly1305 Encrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key", "nonce"],
            "properties": {
                "key": { "type": "string", "description": "Hex-encoded 256-bit key (64 hex chars)" },
                "nonce": { "type": "string", "description": "Hex-encoded 96-bit nonce (24 hex chars)" },
                "aad": { "type": "string", "description": "Hex-encoded additional authenticated data (optional)", "default": "" }
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
        let key_hex = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
            TransformError::InvalidParameter {
                field: "key".into(),
                reason: "hex key required".into(),
            }
        })?;
        let nonce_hex = params
            .get("nonce")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "hex nonce required".into(),
            })?;
        let _aad_hex = params.get("aad").and_then(|v| v.as_str()).unwrap_or("");

        let key = hex::decode(key_hex.trim()).map_err(|e| TransformError::InvalidParameter {
            field: "key".into(),
            reason: format!("invalid hex key: {e}"),
        })?;
        let nonce =
            hex::decode(nonce_hex.trim()).map_err(|e| TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: format!("invalid hex nonce: {e}"),
            })?;
        let aad = hex::decode(
            params
                .get("aad")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim(),
        )
        .unwrap_or_default();

        if key.len() != 32 {
            return Err(TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("ChaCha20-Poly1305 key must be 32 bytes (got {})", key.len()),
            });
        }
        if nonce.len() != 12 {
            return Err(TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "ChaCha20-Poly1305 nonce must be 12 bytes (96 bits)".into(),
            });
        }

        let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|e| {
            TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("ChaCha20-Poly1305 init failed: {e}"),
            }
        })?;
        let nonce = Nonce::from_slice(&nonce);
        let ciphertext = cipher
            .encrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: input.as_ref(),
                    aad: &aad,
                },
            )
            .map_err(|e| {
                TransformError::Internal(format!("ChaCha20-Poly1305 encrypt failed: {e}"))
            })?;

        Ok(Cow::Owned(ciphertext))
    }
}

pub struct ChaCha20Poly1305Decrypt;

impl Transform for ChaCha20Poly1305Decrypt {
    fn id(&self) -> &'static str {
        "crypto.chacha20_poly1305.decrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "ChaCha20-Poly1305 Decrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key", "nonce"],
            "properties": {
                "key": { "type": "string", "description": "Hex-encoded 256-bit key (64 hex chars)" },
                "nonce": { "type": "string", "description": "Hex-encoded 96-bit nonce (24 hex chars)" },
                "aad": { "type": "string", "description": "Hex-encoded additional authenticated data (optional)", "default": "" }
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
        let key_hex = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
            TransformError::InvalidParameter {
                field: "key".into(),
                reason: "hex key required".into(),
            }
        })?;
        let nonce_hex = params
            .get("nonce")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "hex nonce required".into(),
            })?;
        let _aad_hex = params.get("aad").and_then(|v| v.as_str()).unwrap_or("");

        let key = hex::decode(key_hex.trim()).map_err(|e| TransformError::InvalidParameter {
            field: "key".into(),
            reason: format!("invalid hex key: {e}"),
        })?;
        let nonce =
            hex::decode(nonce_hex.trim()).map_err(|e| TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: format!("invalid hex nonce: {e}"),
            })?;
        let aad = hex::decode(
            params
                .get("aad")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim(),
        )
        .unwrap_or_default();

        if key.len() != 32 {
            return Err(TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("ChaCha20-Poly1305 key must be 32 bytes (got {})", key.len()),
            });
        }
        if nonce.len() != 12 {
            return Err(TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "ChaCha20-Poly1305 nonce must be 12 bytes (96 bits)".into(),
            });
        }

        let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|e| {
            TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("ChaCha20-Poly1305 init failed: {e}"),
            }
        })?;
        let nonce = Nonce::from_slice(&nonce);
        let plaintext = cipher
            .decrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: input.as_ref(),
                    aad: &aad,
                },
            )
            .map_err(|e| TransformError::InvalidInput {
                reason: format!("ChaCha20-Poly1305 decrypt failed: {e}"),
            })?;

        Ok(Cow::Owned(plaintext))
    }
}

inventory::submit! { crate::TransformEntry(&ChaCha20Poly1305Encrypt) }
inventory::submit! { crate::TransformEntry(&ChaCha20Poly1305Decrypt) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn chacha20_poly1305_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(32); // 256-bit key
        let nonce = "00".repeat(12); // 96-bit nonce
        let params = serde_json::json!({"key": key, "nonce": nonce});
        let pt = b"Hello ChaCha20-Poly1305!";
        let enc = ChaCha20Poly1305Encrypt
            .apply(Cow::Borrowed(pt), &params, &ctx)
            .unwrap();
        let dec = ChaCha20Poly1305Decrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn chacha20_poly1305_aad_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(32);
        let nonce = "00".repeat(12);
        let aad = "aad data";
        let params = serde_json::json!({"key": key, "nonce": nonce, "aad": aad});
        let pt = b"Data with AAD";
        let enc = ChaCha20Poly1305Encrypt
            .apply(Cow::Borrowed(pt), &params, &ctx)
            .unwrap();
        let dec = ChaCha20Poly1305Decrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn rejects_invalid_key() {
        let ctx = NullExecutionContext;
        let err = ChaCha20Poly1305Encrypt
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"key": "0011", "nonce": "00".repeat(12)}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn rejects_invalid_nonce() {
        let ctx = NullExecutionContext;
        let err = ChaCha20Poly1305Encrypt
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"key": "00".repeat(32), "nonce": "00"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(32);
        let nonce = "00".repeat(12);
        let params = serde_json::json!({"key": key, "nonce": nonce});
        let enc = ChaCha20Poly1305Encrypt
            .apply(Cow::Owned(b"test".to_vec()), &params, &ctx)
            .unwrap();
        // Tamper with ciphertext
        let len = enc.len();
        if len > 0 {
            let mut enc_mut = enc.into_owned();
            enc_mut[len - 1] ^= 0xFF;
            let err = ChaCha20Poly1305Decrypt
                .apply(Cow::Owned(enc_mut), &params, &ctx)
                .unwrap_err();
            assert!(matches!(err, TransformError::InvalidInput { .. }));
        }
    }
}
