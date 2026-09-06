//! Template drift-guard: the example plugin's WIT copy must stay in sync
//! with the canonical host contract.
//!
//! Split of duties (no duplication): artifact/manifest hash sync and the
//! install+execute gates on the committed pair live in `sdk_lifecycle.rs`;
//! this file guards the *contract source* — `plugins/example-wit/wit/plugin.wit`
//! is a hand-maintained copy of `crates/hexforge-plugin-host/wit/plugin.wit`
//! and can drift silently (today the copies differ in comments only).
//! Comparison is semantic: `///` doc lines and blank lines are ignored, so
//! comment wording never fails the guard but any package / interface /
//! function / world change does. No signing, no secrets, no wasm toolchain:
//! plain `cargo test` (which CI already runs) enforces it.

use hexforge_plugin_host::WIT_VERSION;

const HOST_WIT: &str = include_str!("../wit/plugin.wit");
const TEMPLATE_WIT: &str = include_str!("../../../plugins/example-wit/wit/plugin.wit");

/// WIT source with documentation and blank lines removed: two files are
/// "in sync" iff their semantic lines match exactly and in order.
fn semantic_wit(source: &str) -> Vec<&str> {
    source
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim_start().starts_with("///"))
        .filter(|line| !line.trim().is_empty())
        .collect()
}

#[test]
fn template_wit_matches_host_contract() {
    let host = semantic_wit(HOST_WIT);
    let template = semantic_wit(TEMPLATE_WIT);
    assert_eq!(
        template, host,
        "plugins/example-wit/wit/plugin.wit drifted from \
         crates/hexforge-plugin-host/wit/plugin.wit: copy the canonical file, \
         rebuild the template (`cargo build --target wasm32-wasip1`, \
         `wasm-tools component new`), re-bind and re-test"
    );
}

#[test]
fn template_wit_declares_host_version() {
    let package_line = semantic_wit(TEMPLATE_WIT)
        .into_iter()
        .find(|line| line.starts_with("package "))
        .expect("template WIT must declare a package");
    assert_eq!(
        package_line,
        format!("package hexforge:plugin@{WIT_VERSION};"),
        "template WIT package must track the host WIT_VERSION ({WIT_VERSION})"
    );
}

#[test]
fn semantic_wit_ignores_comments_but_catches_code_drift() {
    // Guard self-check on synthetic inputs: proves the comparator bites.
    let a = "package hexforge:plugin@0.1.0;\n/// a doc comment\ninterface transform {}";
    let b = "package hexforge:plugin@0.1.0;\n/// different wording\ninterface transform {}";
    assert_eq!(semantic_wit(a), semantic_wit(b));
    let c = "package hexforge:plugin@0.2.0;\n/// a doc comment\ninterface transform {}";
    assert_ne!(semantic_wit(a), semantic_wit(c));
    let d = "package hexforge:plugin@0.1.0;\ninterface transform {\n apply: func();\n}";
    assert_ne!(semantic_wit(a), semantic_wit(d));
}
