//! `hashing.pbkdf2` — PBKDF2-HMAC key derivation (RFC 8018).
//! Параметры `password`/`salt` — UTF-8 строки, `hash` ∈ {sha1,sha256,sha512},
//! `iterations` ≥1 (default 1000), `length` 1..128 (default 32, hex-output
//! удваивает размер). Входной `ByteView` игнорируется — ключ выводится
//! исключительно из параметров (детерминированная KDF).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::borrow::Cow;

pub struct Pbkdf2Hash;

impl Transform for Pbkdf2Hash {
    fn id(&self) -> &'static str {
        "hashing.pbkdf2"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "PBKDF2"
    }
    fn category(&self) -> &'static str {
        "Hashing"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["password", "salt"],
            "properties": {
                "password": { "type": "string", "description": "Password (utf8)" },
                "salt": { "type": "string", "description": "Salt (utf8)" },
                "iterations": { "type": "integer", "minimum": 1, "default": 1000 },
                "length": { "type": "integer", "minimum": 1, "maximum": 128, "default": 32 },
                "hash": { "type": "string", "enum": ["sha1", "sha256", "sha512"], "default": "sha256" }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::Constant,
        }
    }
    fn apply<'a>(
        &self,
        _input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let password = params
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "password".into(),
                reason: "string parameter 'password' is required".into(),
            })?;
        let salt = params.get("salt").and_then(|v| v.as_str()).ok_or_else(|| {
            TransformError::InvalidParameter {
                field: "salt".into(),
                reason: "string parameter 'salt' is required".into(),
            }
        })?;
        let iterations = params
            .get("iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as u32;
        let length = params.get("length").and_then(|v| v.as_u64()).unwrap_or(32) as usize;
        let hash = params
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256");
        if length == 0 || length > 128 {
            return Err(TransformError::InvalidParameter {
                field: "length".into(),
                reason: "must be 1..128".into(),
            });
        }
        if iterations == 0 {
            return Err(TransformError::InvalidParameter {
                field: "iterations".into(),
                reason: "must be >=1".into(),
            });
        }
        let mut out = vec![0u8; length];
        match hash {
            "sha1" => {
                pbkdf2::pbkdf2_hmac::<Sha1>(
                    password.as_bytes(),
                    salt.as_bytes(),
                    iterations,
                    &mut out,
                );
            }
            "sha256" => {
                pbkdf2::pbkdf2_hmac::<Sha256>(
                    password.as_bytes(),
                    salt.as_bytes(),
                    iterations,
                    &mut out,
                );
            }
            "sha512" => {
                pbkdf2::pbkdf2_hmac::<Sha512>(
                    password.as_bytes(),
                    salt.as_bytes(),
                    iterations,
                    &mut out,
                );
            }
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "hash".into(),
                    reason: "must be sha1|sha256|sha512".into(),
                })
            }
        }
        Ok(Cow::Owned(hex::encode(out).into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Pbkdf2Hash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn pbkdf2_sha256_known() {
        let ctx = NullExecutionContext;
        // PBKDF2-HMAC-SHA256 is deterministic; check length and that different iterations differ
        let out1 = Pbkdf2Hash.apply(Cow::Borrowed(b""), &serde_json::json!({"password":"password","salt":"salt","iterations":1,"length":32,"hash":"sha256"}), &ctx).unwrap();
        let out2 = Pbkdf2Hash.apply(Cow::Borrowed(b""), &serde_json::json!({"password":"password","salt":"salt","iterations":4096,"length":32,"hash":"sha256"}), &ctx).unwrap();
        assert_eq!(out1.len(), 64);
        assert_eq!(out2.len(), 64);
        assert_ne!(out1, out2);
        // Known SHA1 vector (RFC6070) to ensure HMAC wiring is correct for SHA1
        let out_sha1 = Pbkdf2Hash.apply(Cow::Borrowed(b""), &serde_json::json!({"password":"password","salt":"salt","iterations":4096,"length":20,"hash":"sha1"}), &ctx).unwrap();
        assert_eq!(
            out_sha1.as_ref(),
            b"4b007901b765489abead49d926f721d065a429c1"
        );
    }

    #[test]
    fn pbkdf2_sha1_vector() {
        let ctx = NullExecutionContext;
        let out = Pbkdf2Hash.apply(Cow::Borrowed(b""), &serde_json::json!({"password":"password","salt":"salt","iterations":1,"length":20,"hash":"sha1"}), &ctx).unwrap();
        assert_eq!(out.as_ref(), b"0c60c80f961f0e71f3a9b524af6012062fe037a6");
    }

    #[test]
    fn pbkdf2_missing_params() {
        let ctx = NullExecutionContext;
        let err = Pbkdf2Hash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
