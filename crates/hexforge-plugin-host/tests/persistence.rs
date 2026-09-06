//! Persistence lifecycle: `install → persist → restart → rediscover →
//! re-verify → register` (FR-6 persistent lifecycle).
//!
//! Every test uses a FRESH root per case and simulates restart by dropping
//! the first `PluginLibrary` and constructing a new one over the same
//! directory — nothing is trusted from the previous instance, only bytes on
//! disk. A separate `PluginRuntime` executes rediscovered instances to prove
//! they are genuinely registrable, not just listed.

use hexforge_plugin_host::{
    bind_wasm_artifact, generate_keypair, sign_manifest,
    store::{DiscoveredPlugin, PluginLibrary, PluginStatus},
    PluginRuntime,
};
use std::path::{Path, PathBuf};

const ECHO_WAT: &str = "(module)";
const OTHER_WAT: &str = "(module (memory 1))";

struct SignedPackage {
    wasm_bytes: Vec<u8>,
    manifest_bytes: Vec<u8>,
    sig_hex: String,
    pubkey_hex: String,
}

/// Author → bind → sign. Exactly the documented developer workflow; the
/// helper under test is part of the shipped SDK (`bind_wasm_artifact`).
fn signed_package(
    id: &str,
    version: &str,
    requested: &[&str],
    granted: &[&str],
    wat: &str,
) -> SignedPackage {
    let wasm_bytes = wat::parse_str(wat).expect("fixture WAT must assemble");
    let requested_json = requested
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let granted_json = granted
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let authored = format!(
        "{{\"id\":\"{id}\",\"name\":\"{id} name\",\"version\":\"{version}\",\
         \"author\":\"Test\",\"requested_capabilities\":[{requested_json}],\
         \"granted_capabilities\":[{granted_json}]}}"
    );
    let manifest_bytes = bind_wasm_artifact(authored.as_bytes(), &wasm_bytes)
        .expect("bind must succeed on authored JSON");
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(&manifest_bytes, &signing_key_hex).expect("sign must succeed");
    SignedPackage {
        wasm_bytes,
        manifest_bytes,
        sig_hex,
        pubkey_hex,
    }
}

fn fresh_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("hexforge-plib-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn drop_root(root: &PathBuf) {
    let _ = std::fs::remove_dir_all(root);
}

/// Restart simulation: forget everything, re-open the same root.
fn reopen(root: &Path) -> PluginLibrary {
    PluginLibrary::new(root.to_path_buf(), Vec::new())
}

fn package_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

#[test]
fn install_then_discover_verified() {
    let root = fresh_root("install-discover");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.persist", "1.0.0", &[], &[], ECHO_WAT);

    let entry = lib
        .install_package(
            &pkg.wasm_bytes,
            &pkg.manifest_bytes,
            &pkg.sig_hex,
            &pkg.pubkey_hex,
        )
        .expect("valid package must install");
    assert_eq!(entry.status, PluginStatus::Verified);
    assert!(entry.instance.is_some());

    let found = lib.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "test.persist");
    assert_eq!(found[0].status, PluginStatus::Verified);
    drop_root(&root);
}

#[test]
fn restart_rediscovery_registers_executable_plugin() {
    let root = fresh_root("restart-exec");
    let pkg = signed_package("test.rts", "1.0.0", &[], &[], ECHO_WAT);
    {
        let lib = PluginLibrary::new(root.clone(), Vec::new());
        lib.install_package(
            &pkg.wasm_bytes,
            &pkg.manifest_bytes,
            &pkg.sig_hex,
            &pkg.pubkey_hex,
        )
        .unwrap();
    } // forget everything: restart

    let lib2 = reopen(&root);
    let instances = lib2.verified_instances();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].manifest.id, "test.rts");

    // The rediscovered instance genuinely executes (echo core module).
    let runtime = PluginRuntime::new(None).unwrap();
    let out = runtime.execute(&instances[0], b"after restart").unwrap();
    assert_eq!(out, b"after restart");
    drop_root(&root);
}

#[test]
fn tampered_wasm_after_install_fails_closed() {
    let root = fresh_root("tamper-wasm");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.twasm", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();

    // Post-install artifact swap (signature covers the manifest only, so
    // only the wasm_sha256 binding can catch this).
    let evil = wat::parse_str(OTHER_WAT).unwrap();
    std::fs::write(package_dir(&root, "test.twasm").join("plugin.wasm"), &evil).unwrap();

    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status, PluginStatus::Invalid);
    assert!(found[0].instance.is_none());
    assert!(lib2.verified_instances().is_empty());
    drop_root(&root);
}

#[test]
fn tampered_manifest_after_install_fails_closed() {
    let root = fresh_root("tamper-manifest");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.tman", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();

    let manifest_path = package_dir(&root, "test.tman").join("manifest.json");
    let mut bytes = std::fs::read(&manifest_path).unwrap();
    // Flip a byte inside the version string; signature must break.
    let pos = bytes
        .windows(b"1.0.0".len())
        .position(|w| w == b"1.0.0")
        .expect("version present");
    bytes[pos] = b'2';
    std::fs::write(&manifest_path, &bytes).unwrap();

    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found[0].status, PluginStatus::Invalid);
    assert!(found[0].instance.is_none());
    drop_root(&root);
}

#[test]
fn missing_artifact_is_unavailable_not_verified() {
    let root = fresh_root("missing-wasm");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.miss", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();
    std::fs::remove_file(package_dir(&root, "test.miss").join("plugin.wasm")).unwrap();

    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found[0].status, PluginStatus::Unavailable);
    assert!(found[0].instance.is_none());
    assert!(lib2.verified_instances().is_empty());
    drop_root(&root);
}

#[test]
fn invalid_signature_never_persists() {
    let root = fresh_root("bad-sig");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.bsig", "1.0.0", &[], &[], ECHO_WAT);
    let (other_pub, _) = generate_keypair();

    // Wrong key: install refuses before touching disk.
    let err = lib
        .install_package(
            &pkg.wasm_bytes,
            &pkg.manifest_bytes,
            &pkg.sig_hex,
            &other_pub,
        )
        .unwrap_err();
    assert!(
        format!("{err}").contains("signature"),
        "unexpected error: {err}"
    );
    assert!(!package_dir(&root, "test.bsig").exists());
    assert!(lib.discover().is_empty());

    // Garbage signature hex: same fail-closed outcome.
    let err = lib
        .install_package(&pkg.wasm_bytes, &pkg.manifest_bytes, "zz", &pkg.pubkey_hex)
        .unwrap_err();
    assert!(format!("{err}").contains("signature"), "{err}");
    assert!(!package_dir(&root, "test.bsig").exists());
    drop_root(&root);
}

#[test]
fn corrupted_manifest_and_grants_fail_closed() {
    let root = fresh_root("corrupt");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.corr", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();
    let dir = package_dir(&root, "test.corr");

    std::fs::write(dir.join("manifest.json"), b"{ not json").unwrap();
    let lib2 = reopen(&root);
    assert_eq!(lib2.discover()[0].status, PluginStatus::Invalid);

    // Restore manifest, corrupt grants instead.
    std::fs::write(dir.join("manifest.json"), &pkg.manifest_bytes).unwrap();
    std::fs::write(dir.join("grants.json"), b"{ broken").unwrap();
    let lib3 = reopen(&root);
    let found = lib3.discover();
    assert_eq!(found[0].status, PluginStatus::Invalid);
    assert!(
        found[0].error.as_deref().unwrap().contains("grants"),
        "{:?}",
        found[0].error
    );

    // Unknown grants schema version fails closed instead of being misread.
    std::fs::write(dir.join("grants.json"), r#"{"version":999,"granted":[]}"#).unwrap();
    let lib4 = reopen(&root);
    assert_eq!(lib4.discover()[0].status, PluginStatus::Invalid);
    drop_root(&root);
}

#[test]
fn missing_sidecars_fail_closed() {
    let root = fresh_root("nosig");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.nosig", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();
    std::fs::remove_file(package_dir(&root, "test.nosig").join("manifest.json.sig")).unwrap();

    let lib2 = reopen(&root);
    assert_eq!(lib2.discover()[0].status, PluginStatus::Invalid);
    assert!(lib2.verified_instances().is_empty());
    drop_root(&root);
}

#[test]
fn duplicate_install_is_deterministic_replace() {
    let root = fresh_root("replace");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let v1 = signed_package("test.dup", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &v1.wasm_bytes,
        &v1.manifest_bytes,
        &v1.sig_hex,
        &v1.pubkey_hex,
    )
    .unwrap();

    // Same id, new valid version: atomic replace, exactly one entry.
    let v2 = signed_package("test.dup", "1.1.0", &[], &[], OTHER_WAT);
    let entry = lib
        .install_package(
            &v2.wasm_bytes,
            &v2.manifest_bytes,
            &v2.sig_hex,
            &v2.pubkey_hex,
        )
        .expect("valid re-install must replace");
    assert_eq!(entry.status, PluginStatus::Verified);

    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].manifest.as_ref().unwrap().version, "1.1.0");

    // Same-version reinstall is also a clean replace (repair flow).
    let v1b = signed_package("test.dup", "1.1.0", &[], &[], OTHER_WAT);
    lib2.install_package(
        &v1b.wasm_bytes,
        &v1b.manifest_bytes,
        &v1b.sig_hex,
        &v1b.pubkey_hex,
    )
    .unwrap();
    assert_eq!(reopen(&root).discover().len(), 1);
    drop_root(&root);
}

#[test]
fn failed_replace_keeps_old_package_and_no_debris() {
    let root = fresh_root("rollback");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let v1 = signed_package("test.rb", "1.0.0", &[], &[], ECHO_WAT);
    lib.install_package(
        &v1.wasm_bytes,
        &v1.manifest_bytes,
        &v1.sig_hex,
        &v1.pubkey_hex,
    )
    .unwrap();

    // Invalid v2 must not disturb the installed v1.
    let v2 = signed_package("test.rb", "2.0.0", &[], &[], OTHER_WAT);
    let (other_pub, _) = generate_keypair();
    lib.install_package(&v2.wasm_bytes, &v2.manifest_bytes, &v2.sig_hex, &other_pub)
        .unwrap_err();

    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status, PluginStatus::Verified);
    assert_eq!(found[0].manifest.as_ref().unwrap().version, "1.0.0");

    // No staging/backup debris survives as visible entries.
    let names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["test.rb".to_string()], "{names:?}");
    drop_root(&root);
}

#[test]
fn grants_persist_across_restart_and_cannot_escalate() {
    let root = fresh_root("grants");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.gr", "1.0.0", &["network"], &["network"], ECHO_WAT);
    lib.install_package(
        &pkg.wasm_bytes,
        &pkg.manifest_bytes,
        &pkg.sig_hex,
        &pkg.pubkey_hex,
    )
    .unwrap();

    // Revoke persists: fresh library sees the narrowed set.
    lib.set_grants("test.gr", &[]).unwrap();
    let lib2 = reopen(&root);
    let found = lib2.discover();
    assert_eq!(found[0].status, PluginStatus::Verified);
    assert!(found[0]
        .manifest
        .as_ref()
        .unwrap()
        .granted_capabilities
        .is_empty());

    // Re-grant persists too.
    lib2.set_grants("test.gr", &["network".to_string()])
        .unwrap();
    let lib3 = reopen(&root);
    assert_eq!(
        lib3.discover()[0]
            .manifest
            .as_ref()
            .unwrap()
            .granted_capabilities,
        vec!["network".to_string()]
    );

    // Never-requested capability: refused, state untouched.
    let err = lib3
        .set_grants("test.gr", &["filesystem_read".to_string()])
        .unwrap_err();
    assert!(format!("{err}").contains("never requested"), "{err}");

    // Unknown policy capability: refused even if requested on paper.
    let err = lib3.set_grants("test.gr", &["network".to_string(), "root".to_string()]);
    assert!(err.is_err(), "unknown caps must be refused");

    // Unknown plugin id: actionable error, no file created.
    assert!(lib3.set_grants("no.such", &[]).is_err());
    drop_root(&root);
}

#[test]
fn signed_granted_cannot_exceed_requested() {
    // A manifest that ships granted=[network] with requested=[] installs
    // fine (nothing privileged requested) but discovers with EMPTY
    // effective grants: local state can only narrow, never widen.
    let root = fresh_root("narrow");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    let pkg = signed_package("test.nw", "1.0.0", &[], &["network"], ECHO_WAT);
    let entry = lib
        .install_package(
            &pkg.wasm_bytes,
            &pkg.manifest_bytes,
            &pkg.sig_hex,
            &pkg.pubkey_hex,
        )
        .unwrap();
    assert_eq!(entry.status, PluginStatus::Verified);
    assert!(entry
        .manifest
        .as_ref()
        .unwrap()
        .granted_capabilities
        .is_empty());
    drop_root(&root);
}

#[test]
fn tampered_plugin_does_not_break_neighbours() {
    let root = fresh_root("isolation");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    for id in ["test.good-a", "test.good-b"] {
        let pkg = signed_package(id, "1.0.0", &[], &[], ECHO_WAT);
        lib.install_package(
            &pkg.wasm_bytes,
            &pkg.manifest_bytes,
            &pkg.sig_hex,
            &pkg.pubkey_hex,
        )
        .unwrap();
    }
    let evil = wat::parse_str(OTHER_WAT).unwrap();
    std::fs::write(package_dir(&root, "test.good-b").join("plugin.wasm"), &evil).unwrap();

    let lib2 = reopen(&root);
    let mut found = lib2.discover();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].status, PluginStatus::Verified);
    assert_eq!(found[1].status, PluginStatus::Invalid);
    assert_eq!(lib2.verified_instances().len(), 1);

    // And the survivor still executes after the neighbour's failure.
    let runtime = PluginRuntime::new(None).unwrap();
    let out = runtime
        .execute(&lib2.verified_instances()[0], b"isolated")
        .unwrap();
    assert_eq!(out, b"isolated");
    drop_root(&root);
}

#[test]
fn dot_ids_cannot_escape_the_library_root() {
    let root = fresh_root("traversal");
    let lib = PluginLibrary::new(root.clone(), Vec::new());
    for id in [".hidden", ".."] {
        let pkg = signed_package(id, "1.0.0", &[], &[], ECHO_WAT);
        let err = lib
            .install_package(
                &pkg.wasm_bytes,
                &pkg.manifest_bytes,
                &pkg.sig_hex,
                &pkg.pubkey_hex,
            )
            .unwrap_err();
        assert!(format!("{err}").contains("dot-path"), "{id}: {err}");
    }
    assert!(lib.discover().is_empty());
    drop_root(&root);
}

#[test]
fn legacy_flat_pairs_still_discover_from_readonly_root() {
    // Repo-local dev layout (`<name>.wasm` + `<name>.json` + sidecars, no
    // wasm binding): weaker, but install never writes it and discovery
    // keeps listing it — example plugins survive the migration.
    let dev = fresh_root("flat-ro");
    let wasm_bytes = wat::parse_str(ECHO_WAT).unwrap();
    let manifest = br#"{"id":"dev.flat","name":"Dev Flat","version":"1.0.0","author":"Dev","requested_capabilities":[],"granted_capabilities":[]}"#;
    let (pubkey_hex, signing_key_hex) = generate_keypair();
    let sig_hex = sign_manifest(manifest, &signing_key_hex).unwrap();
    std::fs::write(dev.join("flat.wasm"), &wasm_bytes).unwrap();
    std::fs::write(dev.join("flat.json"), manifest).unwrap();
    std::fs::write(dev.join("flat.json.sig"), &sig_hex).unwrap();
    std::fs::write(dev.join("flat.json.pub"), &pubkey_hex).unwrap();

    let empty_writable = fresh_root("flat-w");
    let lib = PluginLibrary::new(empty_writable.clone(), vec![dev.clone()]);
    let found = lib.discover();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "dev.flat");
    assert_eq!(found[0].status, PluginStatus::Verified);

    // Unsigned flat files are skipped, not trusted.
    std::fs::remove_file(dev.join("flat.json.sig")).unwrap();
    assert!(
        PluginLibrary::new(empty_writable.clone(), vec![dev.clone()])
            .verified_instances()
            .is_empty()
    );
    drop_root(&dev);
    drop_root(&empty_writable);
}

#[test]
fn missing_root_discovers_empty() {
    let missing =
        std::env::temp_dir().join(format!("hexforge-plib-absent-{}", uuid::Uuid::new_v4()));
    let lib = PluginLibrary::new(missing, Vec::new());
    assert!(lib.discover().is_empty());
    assert!(lib.verified_instances().is_empty());
    assert!(lib.get("anything").is_none());
}

#[test]
fn status_wire_values_are_stable() {
    assert_eq!(PluginStatus::Verified.as_str(), "verified");
    assert_eq!(PluginStatus::Invalid.as_str(), "invalid");
    assert_eq!(PluginStatus::Unavailable.as_str(), "unavailable");
    assert_eq!(PluginStatus::Incompatible.as_str(), "incompatible");
    let _: DiscoveredPlugin = DiscoveredPlugin {
        id: String::new(),
        manifest: None,
        instance: None,
        status: PluginStatus::Verified,
        error: None,
    };
}
