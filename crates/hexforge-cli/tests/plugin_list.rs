//! `plugin list`: headless twin of the Tauri `list_plugins` command over the
//! same `discover()` backend and fail-closed statuses. Covers the empty
//! library, multi-package sorted output, and a corrupted on-disk artifact
//! surfacing as non-Verified with a reason (mirrors the app startup path,
//! which must show — never silently drop — broken packages).

use hexforge_cli::{
    plugin_bind_artifact, plugin_install, plugin_keygen, plugin_list, plugin_sign_manifest,
};

const TEMPLATE_WASM: &[u8] = include_bytes!("../../../plugins/example-wit/plugin.wasm");
const TEMPLATE_MANIFEST: &str = include_str!("../../../plugins/example-wit/manifest.json");

fn workdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hexforge-ls-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn install_signed(work: &std::path::Path, root: &str, manifest_text: &str) {
    let wasm = work.join("plugin.wasm").to_string_lossy().into_owned();
    let manifest = work.join("manifest.json").to_string_lossy().into_owned();
    std::fs::write(&wasm, TEMPLATE_WASM).unwrap();
    std::fs::write(&manifest, manifest_text).unwrap();
    plugin_bind_artifact(&manifest, &wasm).unwrap();
    let (pubkey, secret) = plugin_keygen();
    let sig = plugin_sign_manifest(&manifest, &secret).unwrap();
    plugin_install(&wasm, &manifest, root, Some(&sig), Some(&pubkey)).unwrap();
}

fn second_manifest() -> String {
    TEMPLATE_MANIFEST.replace(
        "\"id\": \"example.wit-uppercase\"",
        "\"id\": \"example.second\"",
    )
}

#[test]
fn list_empty_library() {
    let work = workdir("empty");
    let root = work.join("library").to_string_lossy().into_owned();
    std::fs::create_dir_all(&root).unwrap();
    let msg = plugin_list(&root).unwrap();
    assert!(msg.contains("no plugins installed"), "{msg}");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn list_shows_installed_packages_sorted() {
    let work = workdir("two");
    let root = work.join("library").to_string_lossy().into_owned();
    install_signed(&work, &root, TEMPLATE_MANIFEST);
    install_signed(&work, &root, &second_manifest());

    let out = plugin_list(&root).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    // Sorted by id: example.second first.
    assert!(lines[0].starts_with("example.second "), "{out}");
    assert!(lines[0].contains("status=Verified"), "{out}");
    assert!(lines[1].starts_with("example.wit-uppercase "), "{out}");
    assert!(lines[1].contains("status=Verified"), "{out}");
    assert!(lines[1].contains("version=1.0.0"), "{out}");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn list_surfaces_corrupted_artifact_as_invalid() {
    let work = workdir("corrupt");
    let root = work.join("library").to_string_lossy().into_owned();
    install_signed(&work, &root, TEMPLATE_MANIFEST);

    // Tamper the STORED artifact post-install (same fail-closed path as a
    // restart discovering a damaged package on disk).
    let stored = std::path::Path::new(&root).join("example.wit-uppercase/plugin.wasm");
    let mut bytes = std::fs::read(&stored).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(&stored, &bytes).unwrap();

    let out = plugin_list(&root).unwrap();
    assert!(out.contains("example.wit-uppercase"), "{out}");
    assert!(!out.contains("status=Verified"), "{out}");
    assert!(out.contains("error="), "{out}");
    let _ = std::fs::remove_dir_all(&work);
}
