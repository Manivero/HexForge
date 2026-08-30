//! `hashing.argon2` — Argon2 KDF (RFC 9106).
//! Параметры `password`/`salt` — UTF-8 строки, `variant` ∈ {argon2id,argon2i,argon2d} (default argon2id),
//! `m_cost` 8..65536 KiB (default 19456), `t_cost` 1..10 (default 2), `p_cost` 1..4 (default 1),
//! `length` 4..128 (default 32, hex-output удваивает). Входной `ByteView` игнорируется.

use argon2::{Algorithm, Argon2, Params as Argon2Params, Version};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct Argon2Hash;

impl Transform for Argon2Hash {
    fn id(&self) -> &'static str {
        "hashing.argon2"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Argon2"
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
                "salt": { "type": "string", "description": "Salt (utf8, >=8 bytes recommended)" },
                "variant": { "type": "string", "enum": ["argon2id", "argon2i", "argon2d"], "default": "argon2id" },
                "m_cost": { "type": "integer", "minimum": 8, "maximum": 65536, "default": 19456, "description": "Memory KiB 8..65536 (DoS cap 64 MiB)" },
                "t_cost": { "type": "integer", "minimum": 1, "maximum": 10, "default": 2 },
                "p_cost": { "type": "integer", "minimum": 1, "maximum": 4, "default": 1 },
                "length": { "type": "integer", "minimum": 4, "maximum": 128, "default": 32 }
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
        let variant = params
            .get("variant")
            .and_then(|v| v.as_str())
            .unwrap_or("argon2id");
        let m_cost = params
            .get("m_cost")
            .and_then(|v| v.as_u64())
            .unwrap_or(19456) as u32;
        let t_cost = params.get("t_cost").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
        let p_cost = params.get("p_cost").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
        let length = params.get("length").and_then(|v| v.as_u64()).unwrap_or(32) as usize;
        if !(8..=65536).contains(&m_cost) {
            return Err(TransformError::InvalidParameter {
                field: "m_cost".into(),
                reason: "must be 8..65536".into(),
            });
        }
        if !(1..=10).contains(&t_cost) {
            return Err(TransformError::InvalidParameter {
                field: "t_cost".into(),
                reason: "must be 1..10".into(),
            });
        }
        if !(1..=4).contains(&p_cost) {
            return Err(TransformError::InvalidParameter {
                field: "p_cost".into(),
                reason: "must be 1..4".into(),
            });
        }
        if !(4..=128).contains(&length) {
            return Err(TransformError::InvalidParameter {
                field: "length".into(),
                reason: "must be 4..128".into(),
            });
        }
        if salt.len() < 4 {
            return Err(TransformError::InvalidParameter {
                field: "salt".into(),
                reason: "salt must be >=4 bytes (8+ recommended)".into(),
            });
        }
        let algo = match variant {
            "argon2id" => Algorithm::Argon2id,
            "argon2i" => Algorithm::Argon2i,
            "argon2d" => Algorithm::Argon2d,
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "variant".into(),
                    reason: "must be argon2id|argon2i|argon2d".into(),
                })
            }
        };
        let argon2_params =
            Argon2Params::new(m_cost, t_cost, p_cost, Some(length)).map_err(|e| {
                TransformError::InvalidParameter {
                    field: "m_cost/t_cost/p_cost/length".into(),
                    reason: format!("invalid argon2 params: {e}"),
                }
            })?;
        let argon2 = Argon2::new(algo, Version::V0x13, argon2_params);
        let mut out = vec![0u8; length];
        argon2
            .hash_password_into(password.as_bytes(), salt.as_bytes(), &mut out)
            .map_err(|e| TransformError::Internal(format!("argon2 failed: {e}")))?;
        Ok(Cow::Owned(hex::encode(out).into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Argon2Hash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn argon2_deterministic() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({"password":"password","salt":"somesalt123","m_cost":8,"t_cost":1,"p_cost":1,"length":32});
        let out1 = Argon2Hash.apply(Cow::Borrowed(b""), &params, &ctx).unwrap();
        let out2 = Argon2Hash.apply(Cow::Borrowed(b""), &params, &ctx).unwrap();
        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 64);
    }

    #[test]
    fn argon2_variants() {
        let ctx = NullExecutionContext;
        for variant in ["argon2id", "argon2i", "argon2d"] {
            let out = Argon2Hash
                .apply(
                    Cow::Borrowed(b""),
                    &serde_json::json!({"password":"p","salt":"saltsalts","variant":variant,"m_cost":8,"t_cost":1,"p_cost":1,"length":16}),
                    &ctx,
                )
                .unwrap();
            assert_eq!(out.len(), 32);
        }
    }

    #[test]
    fn argon2_missing_params() {
        let ctx = NullExecutionContext;
        let err = Argon2Hash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn argon2_salt_too_short() {
        let ctx = NullExecutionContext;
        let err = Argon2Hash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p","salt":"ab","m_cost":8,"t_cost":1,"p_cost":1}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { field, .. } if field == "salt"));
    }

    #[test]
    fn argon2_m_cost_cap() {
        let ctx = NullExecutionContext;
        let err = Argon2Hash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p","salt":"saltsalts","m_cost":999999}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { field, .. } if field == "m_cost"));
    }
}
