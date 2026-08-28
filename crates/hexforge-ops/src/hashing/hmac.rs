use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use sha3::Sha3_256;
use std::borrow::Cow;

pub struct HmacHash;

impl Transform for HmacHash {
    fn id(&self) -> &'static str {
        "hashing.hmac"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "HMAC"
    }
    fn category(&self) -> &'static str {
        "Hashing"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": { "type": "string", "description": "HMAC key (utf8 or hex if hexKey=true)" },
                "hexKey": { "type": "boolean", "default": false, "description": "Interpret key as hex" },
                "hash": { "type": "string", "enum": ["md5", "sha1", "sha256", "sha512", "sha3_256"], "default": "sha256" }
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
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let key_str = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
            TransformError::InvalidParameter {
                field: "key".into(),
                reason: "string parameter 'key' is required".into(),
            }
        })?;
        let hex_key = params
            .get("hexKey")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let hash_name = params
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256");
        let key_bytes = if hex_key {
            let clean: String = key_str
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            hex::decode(&clean).map_err(|e| TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("hexKey decode failed: {e}"),
            })?
        } else {
            key_str.as_bytes().to_vec()
        };
        if key_bytes.is_empty() {
            return Err(TransformError::InvalidParameter {
                field: "key".into(),
                reason: "key must not be empty".into(),
            });
        }
        let data = input.as_ref();
        let hex_out = match hash_name {
            "md5" => {
                let mut mac = Hmac::<Md5>::new_from_slice(&key_bytes)
                    .map_err(|e| TransformError::Internal(format!("hmac init: {e}")))?;
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            "sha1" => {
                let mut mac = Hmac::<Sha1>::new_from_slice(&key_bytes)
                    .map_err(|e| TransformError::Internal(format!("hmac init: {e}")))?;
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            "sha256" => {
                let mut mac = Hmac::<Sha256>::new_from_slice(&key_bytes)
                    .map_err(|e| TransformError::Internal(format!("hmac init: {e}")))?;
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            "sha512" => {
                let mut mac = Hmac::<Sha512>::new_from_slice(&key_bytes)
                    .map_err(|e| TransformError::Internal(format!("hmac init: {e}")))?;
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            "sha3_256" => {
                let mut mac = Hmac::<Sha3_256>::new_from_slice(&key_bytes)
                    .map_err(|e| TransformError::Internal(format!("hmac init: {e}")))?;
                mac.update(data);
                hex::encode(mac.finalize().into_bytes())
            }
            _ => {
                return Err(TransformError::InvalidParameter {
                    field: "hash".into(),
                    reason: "hash must be md5|sha1|sha256|sha512|sha3_256".into(),
                })
            }
        };
        Ok(Cow::Owned(hex_out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&HmacHash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn hmac_sha256_known_vector() {
        // RFC 4231 TC2: key="Jefe", data="what do ya want for nothing?"
        let ctx = NullExecutionContext;
        let out = HmacHash
            .apply(
                Cow::Borrowed(b"what do ya want for nothing?"),
                &serde_json::json!({"key":"Jefe","hash":"sha256"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hmac_md5_known() {
        let ctx = NullExecutionContext;
        let out = HmacHash
            .apply(
                Cow::Borrowed(b"The quick brown fox"),
                &serde_json::json!({"key":"key","hash":"md5"}),
                &ctx,
            )
            .unwrap();
        // Verify length 32 hex chars, deterministic
        assert_eq!(out.len(), 32);
        let out2 = HmacHash
            .apply(
                Cow::Borrowed(b"The quick brown fox"),
                &serde_json::json!({"key":"key","hash":"md5"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out, out2);
    }

    #[test]
    fn hmac_hex_key() {
        let ctx = NullExecutionContext;
        // key hex "4b6579" == "Key"
        let out_hex = HmacHash
            .apply(
                Cow::Borrowed(b"data"),
                &serde_json::json!({"key":"4b6579","hexKey":true,"hash":"sha256"}),
                &ctx,
            )
            .unwrap();
        let out_utf8 = HmacHash
            .apply(
                Cow::Borrowed(b"data"),
                &serde_json::json!({"key":"Key","hash":"sha256"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out_hex, out_utf8);
    }

    #[test]
    fn hmac_empty_key_rejected() {
        let ctx = NullExecutionContext;
        let err = HmacHash
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({"key":""}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn hmac_invalid_hash_rejected() {
        let ctx = NullExecutionContext;
        let err = HmacHash
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({"key":"k","hash":"unknown"}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
