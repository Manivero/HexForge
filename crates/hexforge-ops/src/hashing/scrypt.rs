//! `hashing.scrypt` — scrypt KDF (RFC 7914).
//! Параметры `password`/`salt` — UTF-8 строки, `log_n` 1..15 (N=2^log_n, default 15),
//! `r` 1..8 (default 8), `p` 1..16 (default 1), `length` 1..128 (default 32, hex-output
//! удваивает размер). Входной `ByteView` игнорируется — KDF.

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use scrypt::{scrypt as scrypt_fn, Params as ScryptParams};
use std::borrow::Cow;

pub struct ScryptHash;

impl Transform for ScryptHash {
    fn id(&self) -> &'static str {
        "hashing.scrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "scrypt"
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
                "log_n": { "type": "integer", "minimum": 1, "maximum": 15, "default": 15, "description": "log2(N) 1..15 (N=2^log_n, DoS cap)" },
                "r": { "type": "integer", "minimum": 1, "maximum": 8, "default": 8 },
                "p": { "type": "integer", "minimum": 1, "maximum": 16, "default": 1 },
                "length": { "type": "integer", "minimum": 1, "maximum": 128, "default": 32 }
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
        let log_n = params.get("log_n").and_then(|v| v.as_u64()).unwrap_or(15) as u8;
        let r = params.get("r").and_then(|v| v.as_u64()).unwrap_or(8) as u32;
        let p = params.get("p").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
        let length = params.get("length").and_then(|v| v.as_u64()).unwrap_or(32) as usize;
        if !(1..=15).contains(&log_n) {
            return Err(TransformError::InvalidParameter {
                field: "log_n".into(),
                reason: "must be 1..15".into(),
            });
        }
        if !(1..=8).contains(&r) {
            return Err(TransformError::InvalidParameter {
                field: "r".into(),
                reason: "must be 1..8".into(),
            });
        }
        if !(1..=16).contains(&p) {
            return Err(TransformError::InvalidParameter {
                field: "p".into(),
                reason: "must be 1..16".into(),
            });
        }
        if length == 0 || length > 128 {
            return Err(TransformError::InvalidParameter {
                field: "length".into(),
                reason: "must be 1..128".into(),
            });
        }
        let scrypt_params = ScryptParams::new(log_n, r, p, length).map_err(|e| {
            TransformError::InvalidParameter {
                field: "log_n/r/p/length".into(),
                reason: format!("invalid scrypt params: {e}"),
            }
        })?;
        let mut out = vec![0u8; length];
        scrypt_fn(
            password.as_bytes(),
            salt.as_bytes(),
            &scrypt_params,
            &mut out,
        )
        .map_err(|e| TransformError::Internal(format!("scrypt failed: {e}")))?;
        Ok(Cow::Owned(hex::encode(out).into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&ScryptHash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn scrypt_known_vector() {
        let ctx = NullExecutionContext;
        // scrypt("password", "NaCl", log_n=10, r=8, p=16, len=64) known test vector (RFC 7914)
        let out = ScryptHash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"password","salt":"NaCl","log_n":10,"r":8,"p":16,"length":64}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.len(), 128);
        // first bytes check vs known vector prefix (RFC 7914 section 11)
        let expected_prefix = "fdbabe1c9d3472007856e7190d01e9fe";
        assert!(String::from_utf8_lossy(out.as_ref()).starts_with(expected_prefix));
    }

    #[test]
    fn scrypt_deterministic() {
        let ctx = NullExecutionContext;
        let params =
            serde_json::json!({"password":"p","salt":"s","log_n":4,"r":1,"p":1,"length":16});
        let out1 = ScryptHash.apply(Cow::Borrowed(b""), &params, &ctx).unwrap();
        let out2 = ScryptHash.apply(Cow::Borrowed(b""), &params, &ctx).unwrap();
        assert_eq!(out1, out2);
        assert_eq!(out1.len(), 32);
    }

    #[test]
    fn scrypt_missing_params() {
        let ctx = NullExecutionContext;
        let err = ScryptHash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn scrypt_log_n_cap() {
        let ctx = NullExecutionContext;
        let err = ScryptHash
            .apply(
                Cow::Borrowed(b""),
                &serde_json::json!({"password":"p","salt":"s","log_n":20}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { field, .. } if field == "log_n"));
    }
}
