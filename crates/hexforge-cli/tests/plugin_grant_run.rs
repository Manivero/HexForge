//! CLI parity for `plugin grant` / `revoke` / `run`: same backend paths as
//! the Tauri commands (`PluginLibrary::get`, `set_grants`, fresh discovery,
//! `PluginRuntime::execute`), same persistent store, same policy, no new
//! trust model.
//!
//! Lib-level cycles here; cross-process persistence in
//! `cli_processes_share_persistent_grants` below.

use hexforge_cli::{
    plugin_grant, plugin_install, plugin_keygen, plugin_revoke, plugin_run, plugin_sign_manifest,
};

const TEMPLATE_WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const TEMPLATE_MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");
const PLUGIN_ID: &str = "example.wit-uppercase";

fn workdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hexforge-gr-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Template manifest requesting + self-seeding the `network` grant (signed
/// bytes carry the seed; the installer trusts the out-of-band pubkey).
fn network_manifest() -> String {
    TEMPLATE_MANIFEST
        .replace(
            "\"requested_capabilities\": []",
            "\"requested_capabilities\": [\"network\"]",
        )
        .replace(
            "\"granted_capabilities\": []",
            "\"granted_capabilities\": [\"network\"]",
        )
}

struct Staged {
    root: String,
    work: std::path::PathBuf,
}

fn install_signed(tag: &str, manifest_text: &str) -> (Staged, String, String) {
    let work = workdir(tag);
    let wasm = work.join("plugin.wasm").to_string_lossy().into_owned();
    let manifest = work.join("manifest.json").to_string_lossy().into_owned();
    std::fs::write(&wasm, TEMPLATE_WASM).unwrap();
    std::fs::write(&manifest, manifest_text).unwrap();
    // Bind BEFORE signing: the staged wasm bytes (rebuilt template) pin
    // wasm_sha256; signing then covers the bound manifest.
    hexforge_cli::plugin_bind_artifact(&manifest, &wasm).unwrap();
    let (pubkey, secret) = plugin_keygen();
    let sig = plugin_sign_manifest(&manifest, &secret).unwrap();
    let root = work.join("library").to_string_lossy().into_owned();
    plugin_install(&wasm, &manifest, &root, Some(&sig), Some(&pubkey)).unwrap();
    (Staged { root, work }, sig, pubkey)
}

fn run_text(staged: &Staged, input: &[u8]) -> Vec<u8> {
    let inp = staged.work.join("in.bin").to_string_lossy().into_owned();
    let outp = staged.work.join("out.bin").to_string_lossy().into_owned();
    std::fs::write(&inp, input).unwrap();
    plugin_run(PLUGIN_ID, &staged.root, &inp, &outp).unwrap();
    std::fs::read(&outp).unwrap()
}

#[test]
fn install_grant_run_revoke_cycle() {
    let (staged, _, _) = install_signed("cycle", &network_manifest());

    // Seeded grant at install: executes.
    assert_eq!(run_text(&staged, b"hello"), b"HELLO");

    // Revoke persists: next run is denied with a readable reason.
    let msg = plugin_revoke(PLUGIN_ID, &staged.root, "network").unwrap();
    assert!(msg.contains("effective_grants=[]"), "{msg}");
    let inp = staged.work.join("in2.bin").to_string_lossy().into_owned();
    let outp = staged.work.join("out2.bin").to_string_lossy().into_owned();
    std::fs::write(&inp, b"hello").unwrap();
    let err = plugin_run(PLUGIN_ID, &staged.root, &inp, &outp).unwrap_err();
    assert!(err.contains("not granted"), "{err}");

    // Grant-back restores execution; a fresh library sees the same state.
    let msg = plugin_grant(PLUGIN_ID, &staged.root, "network").unwrap();
    assert!(msg.contains("effective_grants=[\"network\"]"), "{msg}");
    assert_eq!(run_text(&staged, b"hello"), b"HELLO");
    let fresh =
        hexforge_plugin_host::store::PluginLibrary::new(staged.work.join("library"), Vec::new());
    let live: Vec<_> = fresh.verified_instances();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].manifest.granted_capabilities, vec!["network"]);

    // Policy denials stay developer-readable.
    let err = plugin_grant(PLUGIN_ID, &staged.root, "filesystem_read").unwrap_err();
    assert!(err.contains("never requested"), "{err}");
    let err = plugin_grant(PLUGIN_ID, &staged.root, "teleport").unwrap_err();
    assert!(err.contains("unknown capability"), "{err}");

    let _ = std::fs::remove_dir_all(&staged.work);
}

#[test]
fn run_error_paths_are_readable() {
    let (staged, _, _) = install_signed("errors", TEMPLATE_MANIFEST);

    let err = plugin_run("no.such-plugin", &staged.root, "in.bin", "out.bin").unwrap_err();
    assert!(err.contains("unknown plugin"), "{err}");
    let err = plugin_grant("no.such-plugin", &staged.root, "network").unwrap_err();
    assert!(err.contains("unknown plugin"), "{err}");
    let err = plugin_revoke("no.such-plugin", &staged.root, "network").unwrap_err();
    assert!(err.contains("unknown plugin"), "{err}");

    // 6 MiB through the byte loop blows the 10M default fuel budget
    // (input + output fit the 16 MiB guest heap, so fuel goes first).
    let big = vec![b'a'; 6 * 1024 * 1024];
    let inp = staged.work.join("big.bin").to_string_lossy().into_owned();
    let outp = staged.work.join("big.out").to_string_lossy().into_owned();
    std::fs::write(&inp, &big).unwrap();
    let err = plugin_run(PLUGIN_ID, &staged.root, &inp, &outp).unwrap_err();
    assert!(err.contains("fuel exhausted"), "{err}");

    let _ = std::fs::remove_dir_all(&staged.work);
}

/// Separate OS processes sharing one library root: grants persist across
/// invocations, revocation is honored by the next process, and every step
/// reports through real CLI exit codes/streams.
#[test]
fn cli_processes_share_persistent_grants() {
    let work = workdir("procs");
    let manifest_text = network_manifest();
    std::fs::write(work.join("plugin.wasm"), TEMPLATE_WASM).unwrap();
    std::fs::write(work.join("manifest.json"), &manifest_text).unwrap();
    let (pubkey, secret) = plugin_keygen();
    let manifest = work.join("manifest.json").to_string_lossy().into_owned();
    let sig = plugin_sign_manifest(&manifest, &secret).unwrap();

    // CLI binary next to the test executable's target dir.
    let mut bin = std::env::current_exe().unwrap();
    bin.pop(); // strip test binary name
    if bin.ends_with("deps") {
        bin.pop(); // deps/ -> debug|release/
    }
    bin.push(format!("hexforge-cli{}", std::env::consts::EXE_SUFFIX));
    assert!(bin.is_file(), "CLI binary missing: {}", bin.display());

    let root = work.join("library").to_string_lossy().into_owned();
    let wasm = work.join("plugin.wasm").to_string_lossy().into_owned();
    let cli = |args: &[&str]| -> std::process::Output {
        std::process::Command::new(&bin)
            .args(args)
            .output()
            .expect("spawn CLI")
    };

    // p1: install in one process...
    let out = cli(&[
        "plugin", "install", &wasm, &manifest, "--root", &root, "--sig", &sig, "--pub", &pubkey,
    ]);
    assert!(out.status.success(), "install: {out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("Verified"));

    // p2: ...run in another: seeded grant executes.
    std::fs::write(work.join("in.bin"), b"hello").unwrap();
    let inp = work.join("in.bin").to_string_lossy().into_owned();
    let outp = work.join("out.bin").to_string_lossy().into_owned();
    let out = cli(&[
        "plugin", "run", PLUGIN_ID, "--root", &root, "--in", &inp, "--out", &outp,
    ]);
    assert!(out.status.success(), "run: {out:?}");
    assert_eq!(std::fs::read(&outp).unwrap(), b"HELLO");

    // p3: revoke persists...
    let out = cli(&[
        "plugin", "revoke", PLUGIN_ID, "--root", &root, "--cap", "network",
    ]);
    assert!(out.status.success(), "revoke: {out:?}");

    // p4: ...so the next process is denied with a readable reason.
    let out = cli(&[
        "plugin", "run", PLUGIN_ID, "--root", &root, "--in", &inp, "--out", &outp,
    ]);
    assert!(!out.status.success(), "revoked run must fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not granted"),
        "{out:?}"
    );

    // p5/p6: grant-back restores execution across processes.
    let out = cli(&[
        "plugin", "grant", PLUGIN_ID, "--root", &root, "--cap", "network",
    ]);
    assert!(out.status.success(), "grant: {out:?}");
    let out = cli(&[
        "plugin", "run", PLUGIN_ID, "--root", &root, "--in", &inp, "--out", &outp,
    ]);
    assert!(out.status.success(), "re-granted run: {out:?}");
    assert_eq!(std::fs::read(&outp).unwrap(), b"HELLO");

    let _ = std::fs::remove_dir_all(&work);
}
