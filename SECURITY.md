# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |

Only the latest `main` and the latest tagged `0.1.x` are supported for security fixes.

## Reporting a Vulnerability

**Do not open public issues for security concerns.**

- Email the maintainers privately (see `Cargo.toml` authors) or use GitHub's “Report a vulnerability” private advisory for `Manivero/HexForge`.
- Include: affected version/commit, reproduction steps, impact, and if possible a minimal PoC (WASM plugin, recipe JSON, or input file).
- You will receive an acknowledgement within 72 hours and a fix timeline.

We follow coordinated disclosure: we will prepare a fix, add a regression test, and publish a GitHub Security Advisory with `cargo audit` / `npm audit` verification before public disclosure.

## Security Model (as implemented)

- **Offline-first, no network** — production builds have no application-level network access; WebView has no direct filesystem/network permissions (`src-tauri/tauri.conf.json` `csp: "default-src 'self'"`).
- **Typed IPC** — all Tauri commands are `#[tauri::command]` with `#[serde(rename_all = "camelCase")]` DTOs mirrored in `src/lib/ipc-contract.ts`; golden tests in `src-tauri/src/commands.rs` prevent wire-format drift (`05-IPC-CONTRACT.md`).
- **No secrets in repo** — `.env` and `*.pem` are ignored (` .gitignore:15-18`), `.env.example` contains no secrets, `cargo audit` + `npm audit` run in CI (`.github/workflows/ci.yml`).
- **Plugin isolation** — `hexforge-plugin-host` verifies Ed25519 manifests (`verify_signature`), enforces `requested`/`granted` capabilities (`filesystem_read`/`write`/`network`), and runs WASM in `wasmtime` with `consume_fuel(true)` (10M default), `max_wasm_stack(2 MiB)`, `ResourceLimiter` (256 MiB per instance), `Store` per execution, `panic = "unwind"` (`Cargo.toml:30`). See `crates/hexforge-plugin-host/wit/plugin.wit` and `plugins/README.md`.
- **Memory safety** — `SourceEntry::Mapped` uses `memmap2` with documented `unsafe` and `SIGBUS` trade-off (`src-tauri/src/commands.rs:95`), `SourceStore::write_region` is bounds-checked (`WriteRegionError::OutOfBounds`).

## Hardening Checklist (CI)

- `cargo audit --deny warnings` and `npm audit --omit=dev` must be 0 (`cargo audit` DB 1233 advisories, `npm audit` 0).
- `cargo clippy --workspace --all-targets -- -D warnings` must be 0.
- `cargo fmt --check` must be 0.
- Frontend `csp` is `default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'` (`src-tauri/tauri.conf.json:27`).

## Past Fixes

- `quick-xml` 0.31 → 0.41 (`RUSTSEC-2026-0194/0195`), `cargo audit` ignore for Tauri `gtk` unmaintained warnings (documented in `Cargo.toml`).
