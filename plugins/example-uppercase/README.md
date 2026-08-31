# Example Uppercase Plugin (HexForge)

Minimal WASM plugin for HexForge that uppercases ASCII input. Demonstrates the **core module ABI** (`memory` + `transform(input_ptr, input_len) -> output_len`) used by `hexforge-plugin-host`.

## Files

- `manifest.json` — plugin metadata (id, version, author, capabilities). Must be Ed25519-signed for production (see `hexforge-plugin-host` docs).
- `plugin.wat` — WAT source, compiled to `plugin.wasm` via `wat::parse_str` or `wat2wasm`.
- `plugin.wasm` — compiled WASM module (1 page memory, `transform` export).

## ABI

```wat
(memory (export "memory") 1)
(func (export "transform") (param i32 i32) (result i32)
  ;; input at memory[0..input_len], output at memory[0..output_len], returns output_len
)
```

Host (`PluginRuntime::execute_core_module`) writes input bytes at `memory[0]`, calls `transform(0, input_len)`, reads `output_len` bytes from `memory[0]`.

For Component Model WIT (`wit/plugin.wit`), export `transform.apply(input: list<u8>, params: string) -> result<list<u8>, string>` instead — see `crates/hexforge-plugin-host/wit/plugin.wit`.

## Build

```bash
# Requires `wat` crate or `wat2wasm`:
cargo run --quiet -p wat --bin wat2wasm -- plugin.wat -o plugin.wasm
# Or via Rust:
wat::parse_file("plugin.wat") -> Vec<u8>
```

## Install

```bash
# CLI (developer mode, unsigned):
cp plugin.wasm manifest.json ~/.hexforge/plugins/
# Or via Tauri `install_plugin` command with Ed25519 signature:
# manifest.json.sig / manifest.json.pub
```

## Test

The plugin is used in `crates/hexforge-plugin-host` integration tests:
- `plugin_transform_uppercase_via_wasm` — verifies `hello world` → `HELLO WORLD`
- `plugin_transform_reverse_via_wasm` — verifies `abcd` → `dcba`

See `crates/hexforge-plugin-host/src/lib.rs` for WAT examples and `docs/04-RUST-CORE-ARCHITECTURE.md` §7 for isolation (fuel 10M, 2 MiB stack, 256 MiB memory cap, NFR-9).
