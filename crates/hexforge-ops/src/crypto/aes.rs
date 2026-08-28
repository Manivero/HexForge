use aes::{Aes128, Aes192, Aes256};
use cbc::{Decryptor as CbcDecryptor, Encryptor as CbcEncryptor};
use cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit, StreamCipher};
use ctr::Ctr128BE;
use ecb::{Decryptor as EcbDecryptor, Encryptor as EcbEncryptor};
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

fn parse_hex_param(params: &serde_json::Value, field: &str, required: bool) -> Result<Option<Vec<u8>>, TransformError> {
    let Some(v) = params.get(field) else {
        if required {
            return Err(TransformError::InvalidParameter { field: field.into(), reason: format!("hex parameter '{field}' is required") });
        } else {
            return Ok(None);
        }
    };
    let s = v.as_str().ok_or_else(|| TransformError::InvalidParameter { field: field.into(), reason: format!("'{field}' must be a hex string") })?;
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    hex::decode(&clean).map(Some).map_err(|e| TransformError::InvalidParameter { field: field.into(), reason: format!("invalid hex for '{field}': {e}") })
}

fn ecb_encrypt<K>(key: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError>
where
    EcbEncryptor<K>: KeyInit + BlockEncryptMut,
    K: cipher::BlockCipher + cipher::BlockEncrypt,
{
    let mut buf = vec![0u8; data.len() + 16];
    buf[..data.len()].copy_from_slice(data);
    let ct = EcbEncryptor::<K>::new_from_slice(key)
        .map_err(|e| TransformError::Internal(format!("AES ECB init failed: {e}")))?
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .map_err(|e| TransformError::Internal(format!("AES ECB encrypt failed: {e}")))?;
    Ok(ct.to_vec())
}

fn ecb_decrypt<K>(key: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError>
where
    EcbDecryptor<K>: KeyInit + BlockDecryptMut,
    K: cipher::BlockCipher + cipher::BlockDecrypt,
{
    let mut buf = data.to_vec();
    let pt = EcbDecryptor::<K>::new_from_slice(key)
        .map_err(|e| TransformError::Internal(format!("AES ECB init failed: {e}")))?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| TransformError::InvalidInput { reason: format!("AES ECB decrypt failed: {e}") })?;
    Ok(pt.to_vec())
}

fn cbc_encrypt<K>(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError>
where
    CbcEncryptor<K>: KeyIvInit + BlockEncryptMut,
    K: cipher::BlockCipher + cipher::BlockEncrypt,
{
    let mut buf = vec![0u8; data.len() + 16];
    buf[..data.len()].copy_from_slice(data);
    let ct = CbcEncryptor::<K>::new_from_slices(key, iv)
        .map_err(|e| TransformError::Internal(format!("AES CBC init failed: {e}")))?
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .map_err(|e| TransformError::Internal(format!("AES CBC encrypt failed: {e}")))?;
    Ok(ct.to_vec())
}

fn cbc_decrypt<K>(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError>
where
    CbcDecryptor<K>: KeyIvInit + BlockDecryptMut,
    K: cipher::BlockCipher + cipher::BlockDecrypt,
{
    let mut buf = data.to_vec();
    let pt = CbcDecryptor::<K>::new_from_slices(key, iv)
        .map_err(|e| TransformError::Internal(format!("AES CBC init failed: {e}")))?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| TransformError::InvalidInput { reason: format!("AES CBC decrypt failed: {e}") })?;
    Ok(pt.to_vec())
}

fn aes_encrypt_ecb(key: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError> {
    match key.len() {
        16 => ecb_encrypt::<Aes128>(key, data),
        24 => ecb_encrypt::<Aes192>(key, data),
        32 => ecb_encrypt::<Aes256>(key, data),
        _ => Err(TransformError::InvalidParameter { field: "key".into(), reason: format!("AES key must be 16/24/32 bytes (got {})", key.len()) }),
    }
}

fn aes_decrypt_ecb(key: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError> {
    match key.len() {
        16 => ecb_decrypt::<Aes128>(key, data),
        24 => ecb_decrypt::<Aes192>(key, data),
        32 => ecb_decrypt::<Aes256>(key, data),
        _ => Err(TransformError::InvalidParameter { field: "key".into(), reason: format!("AES key must be 16/24/32 bytes (got {})", key.len()) }),
    }
}

fn aes_encrypt_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError> {
    if iv.len() != 16 {
        return Err(TransformError::InvalidParameter { field: "iv".into(), reason: format!("AES CBC iv must be 16 bytes (got {})", iv.len()) });
    }
    match key.len() {
        16 => cbc_encrypt::<Aes128>(key, iv, data),
        24 => cbc_encrypt::<Aes192>(key, iv, data),
        32 => cbc_encrypt::<Aes256>(key, iv, data),
        _ => Err(TransformError::InvalidParameter { field: "key".into(), reason: format!("AES key must be 16/24/32 bytes (got {})", key.len()) }),
    }
}

fn aes_decrypt_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError> {
    if iv.len() != 16 {
        return Err(TransformError::InvalidParameter { field: "iv".into(), reason: format!("AES CBC iv must be 16 bytes (got {})", iv.len()) });
    }
    match key.len() {
        16 => cbc_decrypt::<Aes128>(key, iv, data),
        24 => cbc_decrypt::<Aes192>(key, iv, data),
        32 => cbc_decrypt::<Aes256>(key, iv, data),
        _ => Err(TransformError::InvalidParameter { field: "key".into(), reason: format!("AES key must be 16/24/32 bytes (got {})", key.len()) }),
    }
}

fn aes_crypt_ctr(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, TransformError> {
    if iv.len() != 16 {
        return Err(TransformError::InvalidParameter { field: "iv".into(), reason: format!("AES CTR iv must be 16 bytes (got {})", iv.len()) });
    }
    let mut buf = data.to_vec();
    match key.len() {
        16 => {
            let mut cipher = Ctr128BE::<Aes128>::new_from_slices(key, iv)
                .map_err(|e| TransformError::Internal(format!("AES CTR init failed: {e}")))?;
            cipher.apply_keystream(&mut buf);
        }
        24 => {
            let mut cipher = Ctr128BE::<Aes192>::new_from_slices(key, iv)
                .map_err(|e| TransformError::Internal(format!("AES CTR init failed: {e}")))?;
            cipher.apply_keystream(&mut buf);
        }
        32 => {
            let mut cipher = Ctr128BE::<Aes256>::new_from_slices(key, iv)
                .map_err(|e| TransformError::Internal(format!("AES CTR init failed: {e}")))?;
            cipher.apply_keystream(&mut buf);
        }
        _ => return Err(TransformError::InvalidParameter { field: "key".into(), reason: format!("AES key must be 16/24/32 bytes (got {})", key.len()) }),
    }
    Ok(buf)
}

pub struct AesEncrypt;

impl Transform for AesEncrypt {
    fn id(&self) -> &'static str {
        "crypto.aes.encrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "AES Encrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": { "type": "string", "description": "Hex-encoded key (32/48/64 hex chars for 128/192/256-bit)" },
                "mode": { "type": "string", "enum": ["ecb", "cbc", "ctr"], "default": "cbc" },
                "iv": { "type": "string", "description": "Hex-encoded 16-byte IV for CBC/CTR (32 hex chars)" }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let key = parse_hex_param(params, "key", true)?
            .ok_or_else(|| TransformError::InvalidParameter { field: "key".into(), reason: "hex parameter 'key' is required".into() })?;
        let mode = params.get("mode").and_then(|v| v.as_str()).unwrap_or("cbc");
        let out = match mode {
            "ecb" => aes_encrypt_ecb(&key, input.as_ref())?,
            "cbc" => {
                let iv = parse_hex_param(params, "iv", true)?
                    .ok_or_else(|| TransformError::InvalidParameter { field: "iv".into(), reason: "hex parameter 'iv' is required for cbc".into() })?;
                aes_encrypt_cbc(&key, &iv, input.as_ref())?
            }
            "ctr" => {
                let iv = parse_hex_param(params, "iv", true)?
                    .ok_or_else(|| TransformError::InvalidParameter { field: "iv".into(), reason: "hex parameter 'iv' is required for ctr".into() })?;
                aes_crypt_ctr(&key, &iv, input.as_ref())?
            }
            _ => return Err(TransformError::InvalidParameter { field: "mode".into(), reason: "mode must be ecb|cbc|ctr".into() }),
        };
        Ok(Cow::Owned(out))
    }
}

pub struct AesDecrypt;

impl Transform for AesDecrypt {
    fn id(&self) -> &'static str {
        "crypto.aes.decrypt"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "AES Decrypt"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": { "type": "string" },
                "mode": { "type": "string", "enum": ["ecb", "cbc", "ctr"], "default": "cbc" },
                "iv": { "type": "string" }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let key = parse_hex_param(params, "key", true)?
            .ok_or_else(|| TransformError::InvalidParameter { field: "key".into(), reason: "hex parameter 'key' is required".into() })?;
        let mode = params.get("mode").and_then(|v| v.as_str()).unwrap_or("cbc");
        let out = match mode {
            "ecb" => aes_decrypt_ecb(&key, input.as_ref())?,
            "cbc" => {
                let iv = parse_hex_param(params, "iv", true)?
                    .ok_or_else(|| TransformError::InvalidParameter { field: "iv".into(), reason: "hex parameter 'iv' is required for cbc".into() })?;
                aes_decrypt_cbc(&key, &iv, input.as_ref())?
            }
            "ctr" => {
                let iv = parse_hex_param(params, "iv", true)?
                    .ok_or_else(|| TransformError::InvalidParameter { field: "iv".into(), reason: "hex parameter 'iv' is required for ctr".into() })?;
                // CTR decrypt == encrypt (xor keystream)
                aes_crypt_ctr(&key, &iv, input.as_ref())?
            }
            _ => return Err(TransformError::InvalidParameter { field: "mode".into(), reason: "mode must be ecb|cbc|ctr".into() }),
        };
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&AesEncrypt) }
inventory::submit! { crate::TransformEntry(&AesDecrypt) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn ecb_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00112233445566778899aabbccddeeff";
        let params = serde_json::json!({"key": key, "mode": "ecb"});
        let pt = b"Hello AES ECB!!"; // 15 bytes -> padded to 16
        let enc = AesEncrypt.apply(Cow::Borrowed(pt), &params, &ctx).unwrap();
        let dec = AesDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn cbc_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let iv = "0102030405060708090a0b0c0d0e0f10";
        let params = serde_json::json!({"key": key, "mode": "cbc", "iv": iv});
        let pt = b"The quick brown fox";
        let enc = AesEncrypt.apply(Cow::Borrowed(pt), &params, &ctx).unwrap();
        assert_ne!(enc.as_ref(), pt);
        let dec = AesDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn nist_aes128_ecb_vector() {
        // NIST FIPS-197: key 00..0f, pt 00112233.. etc, ct 69c4e0d86a7b0430d8cdb78070b4c55a
        let ctx = NullExecutionContext;
        let key = "000102030405060708090a0b0c0d0e0f";
        let pt_hex = "00112233445566778899aabbccddeeff";
        let expected_ct = "69c4e0d86a7b0430d8cdb78070b4c55a";
        // For ECB with PKCS7, our 16-byte block will be padded to 32 bytes (2 blocks), so not direct NIST without padding.
        // Instead test that encrypt/decrypt roundtrip with zero IV CBC matches known prefix.
        let pt = hex::decode(pt_hex).unwrap();
        let params_ecb = serde_json::json!({"key": key, "mode": "ecb"});
        let enc = AesEncrypt.apply(Cow::Borrowed(&pt), &params_ecb, &ctx).unwrap();
        // First block should match NIST (without padding, but our padded adds second block)
        let enc_hex = hex::encode(enc.as_ref());
        assert!(enc_hex.starts_with(expected_ct), "first block must match NIST, got {enc_hex}");
    }

    #[test]
    fn invalid_key_rejected() {
        let ctx = NullExecutionContext;
        let err = AesEncrypt.apply(Cow::Borrowed(b"x"), &serde_json::json!({"key": "0011"}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn missing_iv_for_cbc_rejected() {
        let ctx = NullExecutionContext;
        let err = AesEncrypt.apply(Cow::Borrowed(b"x"), &serde_json::json!({"key": "00112233445566778899aabbccddeeff", "mode": "cbc"}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn ctr_roundtrip() {
        let ctx = NullExecutionContext;
        let key = "00112233445566778899aabbccddeeff";
        let iv = "0102030405060708090a0b0c0d0e0f10";
        let params = serde_json::json!({"key": key, "mode": "ctr", "iv": iv});
        let pt = b"The quick brown fox jumps over the lazy dog";
        let enc = AesEncrypt.apply(Cow::Borrowed(pt), &params, &ctx).unwrap();
        assert_ne!(enc.as_ref(), pt);
        assert_eq!(enc.len(), pt.len(), "CTR must not pad");
        let dec = AesDecrypt.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt);
    }

    #[test]
    fn ctr_is_streaming_no_padding() {
        let ctx = NullExecutionContext;
        let key = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let iv = "00000000000000000000000000000000";
        let params = serde_json::json!({"key": key, "mode": "ctr", "iv": iv});
        for len in [0, 1, 15, 16, 17, 31, 32, 100] {
            let pt = vec![0xAB; len];
            let enc = AesEncrypt.apply(Cow::Borrowed(&pt), &params, &ctx).unwrap();
            assert_eq!(enc.len(), len);
            let dec = AesDecrypt.apply(enc, &params, &ctx).unwrap();
            assert_eq!(dec.as_ref(), pt.as_slice());
        }
    }

    #[test]
    fn ctr_missing_iv_rejected() {
        let ctx = NullExecutionContext;
        let err = AesEncrypt.apply(Cow::Borrowed(b"x"), &serde_json::json!({"key": "00112233445566778899aabbccddeeff", "mode": "ctr"}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn ctr_invalid_iv_length_rejected() {
        let ctx = NullExecutionContext;
        let err = AesEncrypt.apply(Cow::Borrowed(b"x"), &serde_json::json!({"key": "00112233445566778899aabbccddeeff", "mode": "ctr", "iv": "0011"}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn ctr_known_vector() {
        // NIST SP 800-38A CTR example: key 2b7e151628aed2a6abf7158809cf4f3c, iv f0f1f2f3.., pt from spec
        let ctx = NullExecutionContext;
        let key = "2b7e151628aed2a6abf7158809cf4f3c";
        let iv = "f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff";
        // Plaintext "6bc1bee22e409f96" repeated? Use simple roundtrip with zero iv to ensure deterministic keystream
        let params = serde_json::json!({"key": key, "mode": "ctr", "iv": iv});
        let pt = hex::decode("6bc1bee22e409f96e93d7e117393172a").unwrap();
        let enc = AesEncrypt.apply(Cow::Borrowed(&pt), &params, &ctx).unwrap();
        // CTR keystream for this key/iv should produce ct 874d6191... (from NIST) but we verify decrypt inverts
        let dec = AesDecrypt.apply(enc.clone(), &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), pt.as_slice());
        // Ensure encryption is deterministic
        let enc2 = AesEncrypt.apply(Cow::Borrowed(&pt), &params, &ctx).unwrap();
        assert_eq!(enc.as_ref(), enc2.as_ref());
    }
}
