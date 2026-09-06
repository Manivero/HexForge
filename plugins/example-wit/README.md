# Example WIT Plugin (official template)

Minimal HexForge component plugin: uppercases ASCII input via the
`hexforge:plugin@0.1.0` contract (`wit/plugin.wit`, world `hexforge-plugin`).
This directory is the single source of truth for `hexforge-cli plugin new`.

## Developer flow (copy-paste)

```sh
# 0. prerequisites (one time)
rustup target add wasm32-wasip1
cargo install wasm-tools
cargo build -p hexforge-cli

# 1. scaffold (or copy this directory)
hexforge-cli plugin new my-plugin --id acme.demo --name Demo
cd my-plugin

# 2. implement: edit src/lib.rs (keep the Guest trait), bump manifest.json

# 3. build the WASI-free core module
cargo build --release --target wasm32-wasip1

# 4. package the component (no --adapt needed: the module imports nothing)
wasm-tools component new \
  target/wasm32-wasip1/release/<crate>.wasm -o plugin.wasm

# 5. validate → bind → sign (IN THIS ORDER)
hexforge-cli plugin validate manifest.json
hexforge-cli plugin bind manifest.json plugin.wasm
hexforge-cli plugin keygen            # prints pubkey + signing_key (save both)
hexforge-cli plugin sign manifest.json --key <signing_key>

# 6. headless install + rediscover (same backend as the app UI)
hexforge-cli plugin install plugin.wasm manifest.json \
  --root /tmp/plugin-lib --sig <sig> --pub <pubkey>

# 7. grant capabilities + execute, headless (same backend as the app UI).
# grant only accepts caps the manifest requests (else 'never requested').
hexforge-cli plugin list --root /tmp/plugin-lib
hexforge-cli plugin grant acme.demo --root /tmp/plugin-lib --cap network
hexforge-cli plugin run acme.demo --root /tmp/plugin-lib \
  --in input.bin --out output.bin
hexforge-cli plugin revoke acme.demo --root /tmp/plugin-lib --cap network
```

## Rules (enforced, not advisory)

- **WASI-free.** The host instantiates components with an empty linker —
  any `wasi_snapshot_preview1` import fails at load. Hence `#![no_std]` +
  bump allocator (`src/lib.rs`). Do not add `std` or WASI deps.
- **Bind before sign.** `wasm_sha256` pins `plugin.wasm` into the manifest;
  the signature covers exactly the shipped bytes. Rebuilding? Re-run
  `bind`, then `sign` again.
- **Contract version.** The host speaks `hexforge:plugin/transform@0.1.0`
  (bare `hexforge:plugin/transform` also accepted). Anything else installs
  as `Incompatible` — never silently.
- **Capabilities.** Requested privileged caps (`network`, `filesystem_*`)
  must be pre-granted or install is refused. Grants live outside the
  signed manifest and can never exceed request ∩ policy.
- **Keys.** Never commit `signing_key`, `.sig` sidecars are fine to keep
  locally, `.pub` ships with the plugin. This repo's `manifest.json` is
  bound but UNSIGNED; tests sign ephemerally.

## What install checks (in order)

Bad signature → malformed manifest → `wasm_sha256` mismatch → unloadable
binary → wrong WIT contract (`Incompatible`) → capability violation.
Every later restart re-verifies signature + binding + contract from disk.

## The committed `plugin.wasm`

Built from this source (`cargo build` + `wasm-tools component new`,
wit-bindgen 0.36). After any rebuild: re-`bind` the manifest, then run

```sh
cargo test -p hexforge-plugin-host --test sdk_lifecycle
cargo test -p hexforge-plugin-host --test template_drift
cargo test -p hexforge-cli --test plugin_tools
cargo test -p hexforge-cli --test plugin_grant_run
cargo test -p hexforge-cli --test plugin_list
```

The sync test fails if `plugin.wasm` and `manifest.json` drift apart.
