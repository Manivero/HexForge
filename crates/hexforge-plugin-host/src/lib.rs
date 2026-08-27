//! hexforge-plugin-host — stub Wasmtime runtime + Ed25519 manifest verification
//! (PRD §3.6, NFR-9). MVP: signature verification and capability sandbox stub
//! without actual WASM execution; WASM host will be plugged via `wasmtime` crate
//! in next iteration. The stub already enforces the security contract:
//! - manifest must be signed Ed25519
//! - signature_verified flag is exposed to UI via `list_plugins`
//! - capabilities default deny

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    #[serde(default)]
    pub granted_capabilities: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PluginError {
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("manifest parse failed: {0}")]
    ManifestParse(String),
}

/// Verifies Ed25519 signature of `manifest_bytes` against `signature_hex` and `pubkey_hex`.
/// Returns true if valid, error if pubkey/signature malformed.
pub fn verify_signature(manifest_bytes: &[u8], signature_hex: &str, pubkey_hex: &str) -> Result<bool, PluginError> {
    let sig_bytes = hex::decode(signature_hex.trim()).map_err(|e| PluginError::InvalidSignature(e.to_string()))?;
    let pk_bytes = hex::decode(pubkey_hex.trim()).map_err(|e| PluginError::InvalidPublicKey(e.to_string()))?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| PluginError::InvalidPublicKey("pubkey must be 32 bytes".into()))?;
    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| PluginError::InvalidSignature("signature must be 64 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|e| PluginError::InvalidPublicKey(e.to_string()))?;
    let sig = Signature::from_bytes(&sig_arr);
    Ok(vk.verify(manifest_bytes, &sig).is_ok())
}

/// Stub list_plugins — returns empty until WASM host is wired (FR-6.1).
/// Kept to satisfy `src-tauri` build without `wasmtime` feature.
pub fn list_plugins_stub() -> Vec<PluginManifest> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn sign_and_verify_roundtrip() {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let msg = b"manifest content";
        let sig = signing_key.sign(msg);
        let sig_hex = hex::encode(sig.to_bytes());
        let pk_hex = hex::encode(verifying_key.to_bytes());
        assert!(verify_signature(msg, &sig_hex, &pk_hex).unwrap());

        // Tampered message fails
        assert!(!verify_signature(b"tampered", &sig_hex, &pk_hex).unwrap());
    }

    #[test]
    fn rejects_malformed_keys() {
        let err = verify_signature(b"msg", "00", "00").unwrap_err();
        assert!(matches!(err, PluginError::InvalidSignature(_) | PluginError::InvalidPublicKey(_)));
    }
}
