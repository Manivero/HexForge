//! Persistent plugin library: `install → validate → verify → persist →
//! restart → rediscover → re-verify → register` (FR-6 lifecycle).
//!
//! Storage format (v1) — one directory per plugin under the writable root:
//! ```text
//! <root>/<plugin-id>/
//!   manifest.json        # signed bytes (EXACT bytes the signature covers)
//!   manifest.json.sig    # lowercase hex Ed25519 signature
//!   manifest.json.pub    # lowercase hex Ed25519 pubkey (TOFU)
//!   plugin.wasm          # artifact, bound via manifest `wasm_sha256`
//!   grants.json          # LOCAL state {"version":1,"granted":[...]} — NEVER signed
//! ```
//! Security guarantees:
//! - Signature is verified BEFORE persist and AGAIN on every discovery;
//!   a plugin is never trusted because it was "once installed".
//! - `plugin.wasm` tampering is detected via `wasm_sha256` (signature covers
//!   the manifest only, so without the binding a post-install `.wasm` swap
//!   would be invisible). The persistent layer REQUIRES the binding; the
//!   legacy flat dev layout (`<name>.wasm` + `<name>.json`) keeps working
//!   without it and is documented as weaker.
//! - `grants.json` can only NARROW: effective grants are always
//!   `grants ∩ requested ∩ POLICY`. A corrupt/mutated grants file rejects the
//!   entry; it can never escalate above the signed `requested` set.
//! - Install never executes WASM: validation parses binaries
//!   (`Component`/`Module::from_binary`) without instantiation. Registration
//!   (`as_transform`, fuel-metered WIT metadata query) reuses the exact same
//!   path as startup rediscovery.
//! - No path traversal: package dir names come from validated manifest ids
//!   (`[A-Za-z0-9._-]`, ≤128 bytes, dot-ids rejected); artifact file names
//!   are constants. Staging uses `<root>/.tmp-<uuid>/` + atomic rename;
//!   dot-directories are never discovered (covers `.tmp-*`/`.bak-*`).
//! - Failure isolation: one bad entry never breaks discovery of the rest;
//!   a failed install leaves neither the package dir nor staging debris.
//!
//! This layer deliberately does NOT touch `PluginRuntime`: validation parses,
//! execution stays where it was.

use super::{validate_manifest, verify_signature, PluginError, PluginInstance, PluginManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// On-disk package format version (`grants.json` schema).
pub const PACKAGE_FORMAT_VERSION: u32 = 1;
/// Env override for the writable library root (tests / portable installs).
pub const PLUGINS_DIR_ENV: &str = "HEXFORGE_PLUGINS_DIR";

const WASM_FILENAME: &str = "plugin.wasm";
const MANIFEST_FILENAME: &str = "manifest.json";
const SIGNATURE_FILENAME: &str = "manifest.json.sig";
const PUBKEY_FILENAME: &str = "manifest.json.pub";
const GRANTS_FILENAME: &str = "grants.json";

/// Privileged capabilities the local policy recognises. Mirrors
/// `PluginRuntime::is_privileged_cap` and `commands::VALID_CAPABILITIES`
/// without duplicating their logic — the effective grant is always
/// intersected with this set at read time.
pub const POLICY_CAPABILITIES: [&str; 3] = ["filesystem_read", "filesystem_write", "network"];

/// Discovery verdict for one entry. Only `Verified` entries are registrable;
/// every other variant carries an actionable `error` for the UI and is NEVER
/// registered or executed (fail-closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    /// Installed, signature re-verified, artifact bound, loadable.
    Verified,
    /// Manifest/signature/grants broken or tampered.
    Invalid,
    /// Manifest references a missing artifact.
    Unavailable,
    /// Binary loads as neither Component nor core module.
    Incompatible,
}

impl PluginStatus {
    /// Stable wire value for the IPC DTO (`status` field).
    pub fn as_str(self) -> &'static str {
        match self {
            PluginStatus::Verified => "verified",
            PluginStatus::Invalid => "invalid",
            PluginStatus::Unavailable => "unavailable",
            PluginStatus::Incompatible => "incompatible",
        }
    }
}

/// One discovery entry: verified entries carry a registrable `instance`,
/// all entries carry an id + status (+ error unless verified).
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub id: String,
    pub manifest: Option<PluginManifest>,
    /// `Some` if and only if `status == Verified`.
    pub instance: Option<PluginInstance>,
    pub status: PluginStatus,
    /// Actionable reason for non-verified entries; file names only, never
    /// absolute host paths.
    pub error: Option<String>,
}

impl DiscoveredPlugin {
    /// Registrable instances for the Transform registry / startup path.
    pub fn verified_instances(entries: &[DiscoveredPlugin]) -> Vec<PluginInstance> {
        entries
            .iter()
            .filter(|e| e.status == PluginStatus::Verified)
            .filter_map(|e| e.instance.clone())
            .collect()
    }
}

/// Local capability state: versioned so future schemas fail closed instead
/// of being misread. NEVER signed — it only narrows the signed `requested`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrantsFile {
    version: u32,
    #[serde(default)]
    granted: Vec<String>,
}

/// Persistent library over one writable root plus any number of read-only
/// roots (repo-local `./plugins` dev discovery). First root wins on id
/// collisions, so a writable install shadows a dev copy deterministically.
pub struct PluginLibrary {
    writable: PathBuf,
    readonly: Vec<PathBuf>,
}

impl PluginLibrary {
    pub fn new(writable: PathBuf, readonly: Vec<PathBuf>) -> Self {
        Self { writable, readonly }
    }

    /// `HEXFORGE_PLUGINS_DIR` when set (tests), otherwise the repo-local
    /// `./plugins` as the single writable root (dev/CLI default).
    /// The Tauri app constructs `new(app_data/plugins, [cwd/plugins])`
    /// explicitly — this crate stays free of the Tauri dependency.
    pub fn from_env_or_cwd() -> Self {
        match std::env::var(PLUGINS_DIR_ENV) {
            Ok(dir) if !dir.trim().is_empty() => Self::new(PathBuf::from(dir), Vec::new()),
            _ => Self::new(PathBuf::from("./plugins"), Vec::new()),
        }
    }

    pub fn writable_dir(&self) -> &Path {
        &self.writable
    }

    /// Full discovery across all roots. Never fails as a whole: every entry
    /// is independently classified (fail-closed per entry, not per scan).
    pub fn discover(&self) -> Vec<DiscoveredPlugin> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut scan_root = |dir: &Path| {
            for entry in self.discover_in_root(dir) {
                if seen.insert(entry.id.clone()) {
                    out.push(entry);
                }
            }
        };
        scan_root(&self.writable.clone());
        for dir in self.readonly.clone() {
            scan_root(&dir);
        }
        out
    }

    /// Shortcut for the startup / install registration path.
    pub fn verified_instances(&self) -> Vec<PluginInstance> {
        DiscoveredPlugin::verified_instances(&self.discover())
    }

    /// Lookup by plugin id across all roots (first root wins).
    pub fn get(&self, plugin_id: &str) -> Option<DiscoveredPlugin> {
        self.discover().into_iter().find(|e| e.id == plugin_id)
    }

    /// Validates a package and atomically persists it into the writable root:
    /// `verify → parse/validate → wasm binding → loadable → policy`, then
    /// stage in `<root>/.tmp-<uuid>/` and publish with rename. NOTHING is
    /// executed (no instantiation). Replaces an existing same-id package only
    /// after the new one is fully staged + re-verified from disk; on any
    /// error neither the old package nor staging debris survives as a
    /// half-installed entry.
    pub fn install_package(
        &self,
        wasm_bytes: &[u8],
        manifest_bytes: &[u8],
        signature_hex: &str,
        pubkey_hex: &str,
    ) -> Result<DiscoveredPlugin, PluginError> {
        let manifest =
            self.validate_new_package(wasm_bytes, manifest_bytes, signature_hex, pubkey_hex)?;
        let id = manifest.id.clone();
        let dest = self.writable.join(&id);

        std::fs::create_dir_all(&self.writable).map_err(|e| {
            PluginError::WasmtimeError(format!("cannot create plugin library: {e}"))
        })?;
        let staging = self.writable.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
        // Best-effort cleanup: a failed install must not leave debris that a
        // later scan could mistake for a package (dot-dirs are skipped
        // anyway, but tidiness is part of the atomicity contract).
        let write_staged = || -> Result<(), PluginError> {
            std::fs::create_dir(&staging)
                .map_err(|e| PluginError::WasmtimeError(format!("cannot stage plugin: {e}")))?;
            let put = |name: &str, bytes: &[u8]| {
                std::fs::write(staging.join(name), bytes)
                    .map_err(|e| PluginError::WasmtimeError(format!("cannot stage {name}: {e}")))
            };
            put(WASM_FILENAME, wasm_bytes)?;
            put(MANIFEST_FILENAME, manifest_bytes)?;
            put(SIGNATURE_FILENAME, signature_hex.trim().as_bytes())?;
            put(PUBKEY_FILENAME, pubkey_hex.trim().as_bytes())?;
            let grants = GrantsFile {
                version: PACKAGE_FORMAT_VERSION,
                granted: effective_grants(&manifest, &manifest.granted_capabilities),
            };
            let grants_json = serde_json::to_string_pretty(&grants).map_err(|e| {
                PluginError::InvalidManifest(format!("cannot encode local grants: {e}"))
            })?;
            put(GRANTS_FILENAME, grants_json.as_bytes())?;
            Ok(())
        };
        if let Err(e) = write_staged() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }

        // Publish: backup existing (if any), rename staged into place.
        let backup = self.writable.join(format!(".bak-{}", uuid::Uuid::new_v4()));
        let had_existing = dest.exists();
        if had_existing {
            if let Err(e) = std::fs::rename(&dest, &backup) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(PluginError::WasmtimeError(format!(
                    "cannot back up existing plugin '{id}': {e}"
                )));
            }
        }
        if let Err(e) = std::fs::rename(&staging, &dest) {
            if had_existing {
                let _ = std::fs::rename(&backup, &dest);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(PluginError::WasmtimeError(format!(
                "cannot publish plugin '{id}': {e}"
            )));
        }

        // Re-verify from disk: the returned entry proves the
        // persist → rediscover loop inside the install call itself.
        match Self::discover_package_dir(&dest) {
            e if e.status == PluginStatus::Verified && e.id == id => {
                if had_existing {
                    let _ = std::fs::remove_dir_all(&backup);
                }
                Ok(e)
            }
            e => {
                // Staged bytes do not re-verify (disk fault?) — roll back.
                let _ = std::fs::remove_dir_all(&dest);
                if had_existing {
                    let _ = std::fs::rename(&backup, &dest);
                }
                Err(PluginError::InvalidSignature(format!(
                    "installed package failed re-verification and was rolled back: {}",
                    e.error.unwrap_or_else(|| "unknown reason".into())
                )))
            }
        }
    }

    /// Persists a grant set for an installed package WITHOUT touching the
    /// signed manifest: re-verifies the package (signature + binding), then
    /// requires `granted ⊆ requested ∩ POLICY` and atomically rewrites
    /// `grants.json`. Returns the effective grant list.
    pub fn set_grants(
        &self,
        plugin_id: &str,
        granted: &[String],
    ) -> Result<Vec<String>, PluginError> {
        let dir = self
            .package_dir(plugin_id)
            .ok_or_else(|| PluginError::InvalidManifest(format!("unknown plugin '{plugin_id}'")))?;
        // Trust nothing on disk: re-verify before accepting state changes.
        // Only a fully verified package accepts new grants — an unavailable
        // or incompatible entry has nothing to grant to.
        let entry = Self::discover_package_dir(&dir);
        if entry.status != PluginStatus::Verified {
            return Err(PluginError::InvalidSignature(format!(
                "plugin '{plugin_id}' is not verified ({}); grants refused",
                entry.error.unwrap_or_else(|| "unknown reason".into())
            )));
        }
        let manifest = entry.manifest.ok_or_else(|| {
            PluginError::InvalidManifest(format!("plugin '{plugin_id}' has no readable manifest"))
        })?;
        for cap in granted {
            if !(manifest.requested_capabilities.iter().any(|r| r == cap)) {
                return Err(PluginError::CapabilityDenied(format!(
                    "capability '{cap}' was never requested by plugin '{plugin_id}'"
                )));
            }
            if !POLICY_CAPABILITIES.contains(&cap.as_str()) {
                return Err(PluginError::CapabilityDenied(format!(
                    "capability '{cap}' is not granted by local policy"
                )));
            }
        }
        let grants = GrantsFile {
            version: PACKAGE_FORMAT_VERSION,
            granted: granted.to_vec(),
        };
        let json = serde_json::to_string_pretty(&grants).map_err(|e| {
            PluginError::InvalidManifest(format!("cannot encode local grants: {e}"))
        })?;
        atomic_write_file(&dir.join(GRANTS_FILENAME), json.as_bytes()).map_err(|e| {
            PluginError::WasmtimeError(format!("cannot persist grants for '{plugin_id}': {e}"))
        })?;
        Ok(effective_grants(&manifest, granted))
    }

    /// Shared validation for a NEW package (install path): signature, manifest
    /// semantics, storage-safe id, wasm binding, loadability, grant policy.
    /// Pure validation — no filesystem writes, no WASM execution.
    fn validate_new_package(
        &self,
        wasm_bytes: &[u8],
        manifest_bytes: &[u8],
        signature_hex: &str,
        pubkey_hex: &str,
    ) -> Result<PluginManifest, PluginError> {
        let valid = verify_signature(manifest_bytes, signature_hex, pubkey_hex)
            .map_err(|e| PluginError::InvalidSignature(format!("signature check failed: {e}")))?;
        if !valid {
            return Err(PluginError::InvalidSignature(
                "signature verification failed: manifest tampered or wrong key".into(),
            ));
        }
        let manifest: PluginManifest = serde_json::from_slice(manifest_bytes)
            .map_err(|e| PluginError::ManifestParse(format!("manifest JSON invalid: {e}")))?;
        validate_manifest(&manifest)?;
        reject_unsafe_id(&manifest.id)?;
        let expected = manifest.wasm_sha256.clone().ok_or_else(|| {
            PluginError::InvalidManifest(
                "field 'wasm_sha256' is required: manifest must bind its plugin.wasm bytes".into(),
            )
        })?;
        let actual = artifact_hash_hex(wasm_bytes);
        if !constant_time_eq_hex(&expected, &actual) {
            return Err(PluginError::InvalidSignature(
                "plugin.wasm does not match the signed manifest (wasm_sha256 mismatch)".into(),
            ));
        }
        if !is_loadable(wasm_bytes) {
            return Err(PluginError::WasmtimeError(
                "wasm file is neither a valid component nor a valid core module".into(),
            ));
        }
        if !component_contract_ok(wasm_bytes) {
            return Err(PluginError::Incompatible(format!(
                "component does not export the hexforge:plugin/transform@{} contract \
                 (with `apply`): built against an unsupported WIT version",
                crate::WIT_VERSION
            )));
        }
        // A package that could never execute must not install: privileged
        // requests need matching effective grants (seeded from the manifest).
        let effective = effective_grants(&manifest, &manifest.granted_capabilities);
        for cap in &manifest.requested_capabilities {
            if POLICY_CAPABILITIES.contains(&cap.as_str()) && !effective.contains(cap) {
                return Err(PluginError::CapabilityDenied(format!(
                    "capability '{cap}' requested but not granted (requested={:?}, granted={:?})",
                    manifest.requested_capabilities, manifest.granted_capabilities
                )));
            }
        }
        Ok(manifest)
    }

    /// Writable-root package dir for an id (direct child match only — the id
    /// charset makes traversal impossible, the direct comparison makes it
    /// structurally impossible).
    fn package_dir(&self, plugin_id: &str) -> Option<PathBuf> {
        if reject_unsafe_id(plugin_id).is_err() {
            return None;
        }
        let dir = self.writable.join(plugin_id);
        (dir.is_dir() && dir.join(MANIFEST_FILENAME).is_file()).then_some(dir)
    }

    fn discover_in_root(&self, dir: &Path) -> Vec<DiscoveredPlugin> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return out, // Missing root = empty library, not an error.
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue; // Staging/backup debris is never a package.
            }
            if path.is_dir() {
                out.push(Self::discover_package_dir(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                out.push(discover_flat_pair(&path));
            }
        }
        out
    }

    /// Reads + fully re-verifies one packaged dir from disk.
    fn discover_package_dir(dir: &Path) -> DiscoveredPlugin {
        let fallback_id = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());
        let fail = |status: PluginStatus, error: String| DiscoveredPlugin {
            id: fallback_id.clone(),
            manifest: None,
            instance: None,
            status,
            error: Some(error),
        };
        let manifest_bytes = match std::fs::read(dir.join(MANIFEST_FILENAME)) {
            Ok(b) => b,
            Err(_) => {
                return fail(
                    PluginStatus::Invalid,
                    format!("package '{fallback_id}' is missing its manifest.json"),
                );
            }
        };
        let manifest: PluginManifest = match serde_json::from_slice(&manifest_bytes) {
            Ok(m) => m,
            Err(e) => {
                return fail(
                    PluginStatus::Invalid,
                    format!("package '{fallback_id}' has a corrupted manifest: {e}"),
                );
            }
        };
        if let Err(e) = validate_manifest(&manifest) {
            return fail(PluginStatus::Invalid, format!("invalid manifest: {e}"));
        }
        let id = manifest.id.clone();
        let fail_as = |status: PluginStatus, error: String| DiscoveredPlugin {
            id: id.clone(),
            manifest: Some(manifest.clone()),
            instance: None,
            status,
            error: Some(error),
        };
        let sig_hex = std::fs::read_to_string(dir.join(SIGNATURE_FILENAME)).unwrap_or_default();
        let pub_hex = std::fs::read_to_string(dir.join(PUBKEY_FILENAME)).unwrap_or_default();
        if sig_hex.trim().is_empty() || pub_hex.trim().is_empty() {
            return fail_as(
                PluginStatus::Invalid,
                format!("package '{id}' is missing its signature sidecars"),
            );
        }
        match verify_signature(&manifest_bytes, sig_hex.trim(), pub_hex.trim()) {
            Ok(true) => {}
            _ => {
                return fail_as(
                    PluginStatus::Invalid,
                    format!("package '{id}' failed signature verification (tampered manifest?)"),
                );
            }
        }
        let wasm_bytes = match std::fs::read(dir.join(WASM_FILENAME)) {
            Ok(b) => b,
            Err(_) => {
                return fail_as(
                    PluginStatus::Unavailable,
                    format!("package '{id}' is missing its plugin.wasm artifact"),
                );
            }
        };
        match manifest.wasm_sha256.clone() {
            Some(expected) => {
                if !constant_time_eq_hex(&expected, &artifact_hash_hex(&wasm_bytes)) {
                    return fail_as(
                        PluginStatus::Invalid,
                        format!(
                            "package '{id}' wasm artifact no longer matches its signed manifest"
                        ),
                    );
                }
            }
            None => {
                return fail_as(
                    PluginStatus::Invalid,
                    format!("package '{id}' manifest does not bind its wasm artifact"),
                );
            }
        }
        if !is_loadable(&wasm_bytes) {
            return fail_as(
                PluginStatus::Incompatible,
                format!("package '{id}' wasm is neither a component nor a core module"),
            );
        }
        if !component_contract_ok(&wasm_bytes) {
            return fail_as(
                PluginStatus::Incompatible,
                format!(
                    "package '{id}' component does not export \
                     the hexforge:plugin/transform@{} contract",
                    crate::WIT_VERSION
                ),
            );
        }
        let effective = match read_grants_file(&dir.join(GRANTS_FILENAME)) {
            Ok(g) => g,
            Err(e) => {
                return fail_as(
                    PluginStatus::Invalid,
                    format!("package '{id}' has corrupted local grants state: {e}"),
                );
            }
        };
        let effective = effective_grants(&manifest, &effective);
        let mut stored = manifest.clone();
        stored.granted_capabilities = effective;
        DiscoveredPlugin {
            id: id.clone(),
            manifest: Some(stored.clone()),
            instance: Some(PluginInstance {
                manifest: stored,
                wasm_path: dir.join(WASM_FILENAME).to_string_lossy().into_owned(),
                pubkey_hex: pub_hex.trim().to_string(),
                signature_hex: sig_hex.trim().to_string(),
            }),
            status: PluginStatus::Verified,
            error: None,
        }
    }
}

/// Legacy flat pair (`<name>.wasm` + `<name>.json` + `.sig`/`.pub` sidecars)
/// for the repo-local dev root. Weaker binding by design (no wasm hash in
/// legacy manifests) — kept for example plugins, never written by install.
fn discover_flat_pair(wasm_path: &Path) -> DiscoveredPlugin {
    let stem = wasm_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    let fail = |status: PluginStatus, error: String| DiscoveredPlugin {
        id: stem.clone(),
        manifest: None,
        instance: None,
        status,
        error: Some(error),
    };
    let manifest_path = wasm_path.with_extension("json");
    let manifest_bytes = match std::fs::read(&manifest_path) {
        Ok(b) => b,
        Err(_) => {
            return fail(
                PluginStatus::Unavailable,
                format!("plugin '{stem}' is missing its manifest.json"),
            );
        }
    };
    let manifest: PluginManifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(m) => m,
        Err(e) => {
            return fail(
                PluginStatus::Invalid,
                format!("plugin '{stem}' manifest is corrupted: {e}"),
            )
        }
    };
    if let Err(e) = validate_manifest(&manifest) {
        return fail(PluginStatus::Invalid, format!("invalid manifest: {e}"));
    }
    let id = manifest.id.clone();
    let fail_as = |status: PluginStatus, error: String| DiscoveredPlugin {
        id: id.clone(),
        manifest: Some(manifest.clone()),
        instance: None,
        status,
        error: Some(error),
    };
    let sig_path = manifest_path.with_extension("json.sig");
    let pub_path = manifest_path.with_extension("json.pub");
    let (sig_hex, pub_hex) = match (
        std::fs::read_to_string(&sig_path),
        std::fs::read_to_string(&pub_path),
    ) {
        (Ok(s), Ok(p)) => (s.trim().to_string(), p.trim().to_string()),
        _ => {
            return fail_as(
                PluginStatus::Invalid,
                format!("plugin '{id}' is unsigned (missing .sig/.pub sidecars)"),
            );
        }
    };
    if !verify_signature(&manifest_bytes, &sig_hex, &pub_hex).unwrap_or(false) {
        return fail_as(
            PluginStatus::Invalid,
            format!("plugin '{id}' failed signature verification (tampered manifest?)"),
        );
    }
    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(b) => b,
        Err(_) => {
            return fail_as(
                PluginStatus::Unavailable,
                format!("plugin '{id}' wasm artifact is unreadable"),
            );
        }
    };
    if !is_loadable(&wasm_bytes) {
        return fail_as(
            PluginStatus::Incompatible,
            format!("plugin '{id}' wasm is neither a component nor a core module"),
        );
    }
    // Legacy entries have no local grants file: effective = manifest seed.
    let mut stored = manifest.clone();
    stored.granted_capabilities = effective_grants(&manifest, &manifest.granted_capabilities);
    DiscoveredPlugin {
        id: id.clone(),
        manifest: Some(stored.clone()),
        instance: Some(PluginInstance {
            manifest: stored,
            wasm_path: wasm_path.to_string_lossy().into_owned(),
            pubkey_hex: pub_hex,
            signature_hex: sig_hex,
        }),
        status: PluginStatus::Verified,
        error: None,
    }
}

/// Lowercase hex SHA-256 over artifact bytes.
fn artifact_hash_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Constant-time hex comparison so a mismatch oracle leaks nothing about the
/// expected digest through timing.
fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim().to_lowercase(), b.trim().to_lowercase());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0 && a.len() == 64 && a.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Effective local grants: intersection with the signed `requested` set and
/// the local policy. Corrupt entries (blank/unknown) are dropped, never
/// promoted.
fn effective_grants(manifest: &PluginManifest, granted: &[String]) -> Vec<String> {
    granted
        .iter()
        .filter(|g| !g.trim().is_empty())
        .filter(|g| POLICY_CAPABILITIES.contains(&g.as_str()))
        .filter(|g| manifest.requested_capabilities.iter().any(|r| r == *g))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Reads the local grants file: missing = legacy/empty (no local state yet),
/// present-but-unreadable = corruption signal for the caller.
fn read_grants_file(path: &Path) -> Result<Vec<String>, String> {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("cannot read grants.json: {e}")),
        Ok(bytes) => {
            let file: GrantsFile = serde_json::from_slice(&bytes)
                .map_err(|e| format!("grants.json is not valid JSON: {e}"))?;
            if file.version != PACKAGE_FORMAT_VERSION {
                return Err(format!(
                    "grants.json version {} is not supported (expected {})",
                    file.version, PACKAGE_FORMAT_VERSION
                ));
            }
            Ok(file.granted)
        }
    }
}

/// Storage-safety on top of `validate_manifest`'s charset: the id becomes a
/// directory name, so `.` / `..` / dot-prefixed ids are rejected even though
/// they pass the charset.
fn reject_unsafe_id(id: &str) -> Result<(), PluginError> {
    if id == "." || id == ".." || id.starts_with('.') {
        return Err(PluginError::InvalidManifest(format!(
            "field 'id' must not be a dot-path (id={id:?})"
        )));
    }
    Ok(())
}

/// Component first (production path), core module second (legacy compat).
/// Parsing only — never instantiates, so discovery/install cannot execute.
fn is_loadable(wasm_bytes: &[u8]) -> bool {
    let engine = match wasmtime::Engine::new(&wasmtime::Config::new()) {
        Ok(e) => e,
        Err(_) => return false,
    };
    wasmtime::component::Component::from_binary(&engine, wasm_bytes).is_ok()
        || wasmtime::Module::from_binary(&engine, wasm_bytes).is_ok()
}

/// WIT contract-version gate: a *component* must export the transform
/// instance pinned by this host (`hexforge:plugin/transform`, bare for
/// hand-written fixtures or `@{WIT_VERSION}` as emitted by wit-bindgen)
/// with an `apply` func. Core modules skip the check (legacy compat path —
/// `is_loadable` still gates them). Query-only: never instantiates, so
/// discovery/install cannot execute guest code.
fn component_contract_ok(wasm_bytes: &[u8]) -> bool {
    const TRANSFORM_IFACE: &str = "hexforge:plugin/transform";
    let engine = match wasmtime::Engine::new(&wasmtime::Config::new()) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let component = match wasmtime::component::Component::from_binary(&engine, wasm_bytes) {
        Ok(c) => c,
        Err(_) => return true, // not a component: legacy core-module path
    };
    let iface = component
        .get_export_index(None, TRANSFORM_IFACE)
        .or_else(|| {
            component.get_export_index(None, format!("{TRANSFORM_IFACE}@{}", crate::WIT_VERSION))
        });
    let Some(iface) = iface else {
        return false;
    };
    component.get_export_index(Some(&iface), "apply").is_some()
}

/// Same-dir temp file + rename: atomic on both POSIX and Windows (file
/// rename over an existing file is atomic on Windows; dirs are published by
/// the caller with the backup protocol).
fn atomic_write_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dest.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}
