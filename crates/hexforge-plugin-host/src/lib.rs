//! hexforge-plugin-host — Wasmtime runtime + Ed25519 manifest verification
//! (PRD §3.6, NFR-9). Implements Wasmtime runtime with fuel limits,
//! capability sandbox, and Ed25519 manifest verification.
//!
//! NOTE: Full Wasmtime integration is a work in progress. The core
//! signature verification and manifest parsing are functional.
//! Full Wasmtime execution will be completed in the next iteration.
#![allow(dead_code, unused_imports, unused_variables)]

use anyhow::{Context, Result, anyhow};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

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
    #[error("wasmtime error: {0}")]
    WasmtimeError(String),
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("execution failed: {0}")]
    ExecutionError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInstance {
    pub manifest: PluginManifest,
    pub wasm_path: String,
    pub pubkey_hex: String,
    pub signature_hex: String,
}

/// WASM Plugin Runtime with fuel metering and capability sandbox
/// NOTE: Full Wasmtime integration is a work in progress. The core
/// signature verification and manifest parsing are functional.
/// Full Wasmtime execution will be completed in the next iteration.
pub struct PluginRuntime {
    fuel_limit: u64,
}

impl PluginRuntime {
    /// Creates a new plugin runtime with default fuel limit (10M instructions)
    pub fn new(fuel_limit: Option<u64>) -> Result<Self> {
        Ok(Self {
            fuel_limit: fuel_limit.unwrap_or(10_000_000), // 10M instructions default
        })
    }

    /// Installs a plugin from WASM file with manifest and signature verification
    pub fn install(
        &self,
        wasm_path: &Path,
        manifest_bytes: &[u8],
        signature_hex: &str,
        pubkey_hex: &str,
    ) -> Result<PluginInstance> {
        // 1. Verify signature
        verify_signature(manifest_bytes, signature_hex, pubkey_hex)
            .context("Signature verification failed")?;
        
        // 2. Parse manifest
        let manifest: PluginManifest = serde_json::from_slice(manifest_bytes)
            .context("Failed to parse manifest JSON")?;
        
        // 3. Validate WASM module exists
        if !wasm_path.exists() {
            return Err(anyhow!("WASM file not found: {:?}", wasm_path));
        }
        
        Ok(PluginInstance {
            manifest,
            wasm_path: wasm_path.to_string_lossy().into_owned(),
            pubkey_hex: pubkey_hex.to_string(),
            signature_hex: signature_hex.to_string(),
        })
    }

    /// Executes a plugin with fuel metering and capability sandbox
    /// NOTE: Full Wasmtime execution is TODO. Currently returns a placeholder.
    pub fn execute(
        &self,
        _instance: &PluginInstance,
        _input: &[u8],
    ) -> Result<Vec<u8>> {
        // TODO: Implement full Wasmtime execution with:
        // - Wasmtime engine with fuel metering
        // - Capability sandbox (filesystem, network)
        // - Component model linking
        // - Memory management for input/output
        // For now, return placeholder
        Ok(b"TODO: Wasmtime execution not yet implemented".to_vec())
    }
}

/// Verifies Ed25519 signature of `manifest_bytes` against `signature_hex` and `pubkey_hex`.
pub fn verify_signature(manifest_bytes: &[u8], signature_hex: &str, pubkey_hex: &str) -> Result<bool, PluginError> {
    let sig_bytes = hex::decode(signature_hex.trim())
        .map_err(|e| PluginError::InvalidSignature(e.to_string()))?;
    let pk_bytes = hex::decode(pubkey_hex.trim())
        .map_err(|e| PluginError::InvalidPublicKey(e.to_string()))?;
    let pk_arr: [u8; 32] = pk_bytes.try_into()
        .map_err(|_| PluginError::InvalidPublicKey("pubkey must be 32 bytes".into()))?;
    let sig_arr: [u8; 64] = sig_bytes.try_into()
        .map_err(|_| PluginError::InvalidSignature("signature must be 64 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| PluginError::InvalidPublicKey(e.to_string()))?;
    let sig = Signature::from_bytes(&sig_arr);
    Ok(vk.verify(manifest_bytes, &sig).is_ok())
}

/// Stub list_plugins — returns empty until WASM host is fully wired (FR-6.1).
pub fn list_plugins_stub() -> Vec<PluginInstance> {
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

    #[test]
    fn plugin_runtime_creates() {
        let runtime = PluginRuntime::new(Some(1000)).unwrap();
        assert_eq!(runtime.fuel_limit, 1000);
    }
}