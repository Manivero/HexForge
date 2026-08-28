//! `crypto.aes_gcm` — AES-GCM authenticated encryption (RFC 5116 / NIST SP 800-38D).
//! Uses `aes-gcm` crate with 128/256-bit keys, 96-bit nonce, 128-bit auth tag.

use aes_gcm::aead::Payload;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes128Gcm, Aes256Gcm, Nonce,
};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct AesGcmEncrypt;

impl Transform for AesGcmEncrypt {
    fn id(&self) -> &'static str {
        "crypto.aes_gcm.encrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "AES-GCM Encrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key", "nonce"],
            "properties": {
                "key": { "type": "string", "description": "Hex-encoded key (32/64 hex chars for 128/256-bit)" },
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

        if nonce.len() != 12 {
            return Err(TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "nonce must be 12 bytes (96 bits)".into(),
            });
        }

        let ciphertext = match key.len() {
            16 => {
                let cipher = Aes128Gcm::new_from_slice(&key).map_err(|e| {
                    TransformError::InvalidParameter {
                        field: "key".into(),
                        reason: format!("AES-128 key error: {e}"),
                    }
                })?;
                let nonce = Nonce::from_slice(&nonce);
                cipher
                    .encrypt(
                        nonce,
                        Payload {
                            msg: input.as_ref(),
                            aad: &aad,
                        },
                    )
                    .map_err(|e| TransformError::Internal(format!("AES-GCM encrypt failed: {e}")))?
            }
            32 => {
                let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| {
                    TransformError::InvalidParameter {
                        field: "key".into(),
                        reason: format!("AES-256 key error: {e}"),
                    }
                })?;
                let nonce = Nonce::from_slice(&nonce);
                cipher
                    .encrypt(
                        nonce,
                        Payload {
                            msg: input.as_ref(),
                            aad: &aad,
                        },
                    )
                    .map_err(|e| TransformError::Internal(format!("AES-GCM encrypt failed: {e}")))?
            }
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "key".into(),
                    reason: format!("AES key must be 16 or 32 bytes (got {})", key.len()),
                })
            }
        };

        // Return ciphertext + auth tag (aes-gcm returns combined ciphertext+tag)
        Ok(Cow::Owned(ciphertext))
    }
}

pub struct AesGcmDecrypt;

impl Transform for AesGcmDecrypt {
    fn id(&self) -> &'static str {
        "crypto.aes_gcm.decrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "AES-GCM Decrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key", "nonce"],
            "properties": {
                "key": { "type": "string", "description": "Hex-encoded key (32/64 hex chars for 128/256-bit)" },
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

        if nonce.len() != 12 {
            return Err(TransformError::InvalidParameter {
                field: "nonce".into(),
                reason: "nonce must be 12 bytes (96 bits)".into(),
            });
        }

        let plaintext = match key.len() {
            16 => {
                let cipher = Aes128Gcm::new_from_slice(&key).map_err(|e| {
                    TransformError::InvalidParameter {
                        field: "key".into(),
                        reason: format!("AES-128 key error: {e}"),
                    }
                })?;
                let nonce = Nonce::from_slice(&nonce);
                cipher
                    .decrypt(
                        nonce,
                        Payload {
                            msg: input.as_ref(),
                            aad: &aad,
                        },
                    )
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("AES-GCM decrypt failed: {e}"),
                    })?
            }
            32 => {
                let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| {
                    TransformError::InvalidParameter {
                        field: "key".into(),
                        reason: format!("AES-256 key error: {e}"),
                    }
                })?;
                let nonce = Nonce::from_slice(&nonce);
                cipher
                    .decrypt(
                        nonce,
                        Payload {
                            msg: input.as_ref(),
                            aad: &aad,
                        },
                    )
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("AES-GCM decrypt failed: {e}"),
                    })?
            }
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "key".into(),
                    reason: format!("AES key must be 16 or 32 bytes (got {})", key.len()),
                })
            }
        };

        Ok(Cow::Owned(plaintext))
    }
}

inventory::submit! { crate::TransformEntry(&AesGcmEncrypt) }
inventory::submit! { crate::TransformEntry(&AesGcmDecrypt) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn aes_gcm_128_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(16); // 128-bit key
        let nonce = "00".repeat(12); // 96-bit nonce
        let params = serde_json::json!({"key": key, "nonce": nonce});
        let pt = b"Hello AES-GCM!";
        let enc = AesGcmEncrypt
            .apply(Cow::Borrowed(pt), &params, &ctx)
            .unwrap();
        let dec = AesGcmDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn aes_gcm_256_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(32); // 256-bit key
        let nonce = "01".repeat(12);
        let params = serde_json::json!({"key": key, "nonce": nonce});
        let pt = b"Test AES-256-GCM roundtrip";
        let enc = AesGcmEncrypt
            .apply(Cow::Borrowed(pt), &params, &ctx)
            .unwrap();
        let dec = AesGcmDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn aes_gcm_aad_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(16);
        let nonce = "00".repeat(12);
        let aad = "deadbeef";
        let params = serde_json::json!({"key": key, "nonce": nonce, "aad": aad});
        let pt = b"Data with AAD";
        let enc = AesGcmEncrypt
            .apply(Cow::Borrowed(pt), &params, &ctx)
            .unwrap();
        let dec = AesGcmDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn aes_gcm_rejects_wrong_tag() {
        let ctx = NullExecutionContext;
        let key = "00".repeat(16);
        let nonce = "00".repeat(12);
        let params = serde_json::json!({"key": key, "nonce": nonce});
        let enc = AesGcmEncrypt
            .apply(Cow::Owned(b"test".to_vec()), &params, &ctx)
            .unwrap();
        // Corrupt the auth tag (last 16 bytes)
        let len = enc.len();
        if len >= 16 {
            let mut enc_mut = enc.into_owned();
            enc_mut[len - 1] ^= 0xFF;
            let err = AesGcmDecrypt
                .apply(Cow::Owned(enc_mut), &params, &ctx)
                .unwrap_err();
            assert!(matches!(err, TransformError::InvalidInput { .. }));
        }
    }
}
