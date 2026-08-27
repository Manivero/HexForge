use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

#[allow(clippy::needless_range_loop)]
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = [0; 256];
    for i in 0..256 {
        s[i] = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let mut i: u8 = 0;
    let mut j: u8 = 0;
    let mut out = Vec::with_capacity(data.len());
    for &b in data {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        let k = s[(s[i as usize] as u16 + s[j as usize] as u16) as usize % 256];
        out.push(b ^ k);
    }
    out
}

pub struct Rc4;

impl Transform for Rc4 {
    fn id(&self) -> &'static str {
        "crypto.rc4"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "RC4"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": { "type": "string", "description": "UTF-8 key, or hex if 'hexKey' true" },
                "hexKey": { "type": "boolean", "default": false }
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
        let key_str = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| TransformError::InvalidParameter {
            field: "key".into(),
            reason: "string parameter 'key' is required".into(),
        })?;
        if key_str.is_empty() {
            return Err(TransformError::InvalidParameter { field: "key".into(), reason: "key must not be empty".into() });
        }
        let hex_key = params.get("hexKey").and_then(|v| v.as_bool()).unwrap_or(false);
        let key_bytes = if hex_key {
            let clean: String = key_str.chars().filter(|c| !c.is_whitespace()).collect();
            hex::decode(&clean).map_err(|e| TransformError::InvalidParameter {
                field: "key".into(),
                reason: format!("hexKey decode failed: {e}"),
            })?
        } else {
            key_str.as_bytes().to_vec()
        };
        if key_bytes.is_empty() {
            return Err(TransformError::InvalidParameter { field: "key".into(), reason: "decoded key is empty".into() });
        }
        Ok(Cow::Owned(rc4(&key_bytes, input.as_ref())))
    }
}

inventory::submit! { crate::TransformEntry(&Rc4) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn known_vector_key_plaintext() {
        // RFC 6229 / Wikipedia: RC4("Key", "Plaintext") = BBF316E8D940AF0AD3
        let ctx = NullExecutionContext;
        let out = Rc4
            .apply(
                Cow::Borrowed(b"Plaintext"),
                &serde_json::json!({"key": "Key"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(hex::encode(out.as_ref()), "bbf316e8d940af0ad3");
    }

    #[test]
    fn rc4_is_involution() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({"key": "secret"});
        let enc = Rc4.apply(Cow::Borrowed(b"Hello HexForge"), &params, &ctx).unwrap();
        let dec = Rc4.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), b"Hello HexForge");
    }

    #[test]
    fn hex_key_mode() {
        let ctx = NullExecutionContext;
        // key hex "4b6579" == "Key"
        let out = Rc4
            .apply(
                Cow::Borrowed(b"Plaintext"),
                &serde_json::json!({"key": "4b6579", "hexKey": true}),
                &ctx,
            )
            .unwrap();
        assert_eq!(hex::encode(out.as_ref()), "bbf316e8d940af0ad3");
    }

    #[test]
    fn empty_key_rejected() {
        let ctx = NullExecutionContext;
        let err = Rc4.apply(Cow::Borrowed(b"x"), &serde_json::json!({"key": ""}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
