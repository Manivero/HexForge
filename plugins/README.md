# HexForge Plugins

This directory is scanned by `hexforge-plugin-host` at startup (`list_plugins()`).

Each plugin is a directory or a pair of files:

```
plugins/
  example-uppercase/
    manifest.json   # PluginManifest (id, name, version, author, requested_capabilities)
    plugin.wasm     # WASM module (core) or component (WIT `hexforge:plugin/transform`)
    plugin.wat      # optional WAT source
```

## Manifest

```json
{
  "id": "example.uppercase",
  "name": "Example Uppercase",
  "version": "1.0.0",
  "author": "HexForge Example",
  "requested_capabilities": [],
  "granted_capabilities": []
}
```

For production, `manifest.json` must be Ed25519-signed. The host verifies `manifest.json` bytes against `manifest.json.sig` (hex) and `manifest.json.pub` (hex pubkey). Unsigned plugins load only in developer mode (future).

See `crates/hexforge-plugin-host/src/lib.rs` (`verify_signature`, `PluginRuntime::install`) and `crates/hexforge-plugin-host/wit/plugin.wit` for the WIT `transform` interface (Component Model).

## ABI

### Core module (MVP, used by `example-uppercase`)

- Exports `memory` (1 page) and `transform(input_ptr: i32, input_len: i32) -> i32` (output_len, output at `memory[0]`).
- Host writes input at `memory[0]`, calls `transform(0, len)`, reads `output_len` bytes from `memory[0]`.
- Fuel 10M, stack 2 MiB, memory cap 256 MiB (NFR-9), empty linker = sandbox (no WASI).

### Component Model (WIT, `wit/plugin.wit`)

```wit
package hexforge:plugin@0.1.0;
interface transform {
    get-id: func() -> string;
    get-version: func() -> string;
    get-display-name: func() -> string;
    get-category: func() -> string;
    get-params-schema: func() -> string;
    get-capabilities: func() -> capabilities;
    apply: func(input: list<u8>, params: string) -> result<list<u8>, string>;
}
world hexforge-plugin { export transform; }
```

Host (`PluginRuntime::execute_component`) instantiates the component via `wasmtime::component::Component`, calls `transform.apply`. If the file is not a component, it falls back to core module.

See `crates/hexforge-plugin-host/src/lib.rs` for `PluginTransform` (implements `hexforge_core::Transform`) and `PluginRuntime::with_memory_limit`.

## Capabilities

- `filesystem_read`, `filesystem_write`, `network` — privileged, must be in `requested_capabilities` and `granted_capabilities` (user confirms via `CapabilityGrantDialog`).
- Host checks at `install` and before `execute` (`check_capabilities`). Without grant, `install` fails with `CapabilityDenied`.

## Fuel and isolation

- `PluginRuntime::new(Some(fuel))` — default 10M, `max_wasm_stack(2 MiB)`, `ResourceLimiter` 256 MiB.
- `execute` creates a fresh `Store` per call, sets fuel, instantiates, calls `transform` or `run`, handles `fuel exhausted` and `trap` as `PluginError::WasmtimeError`, never unwinds host (`panic = "unwind"` in `Cargo.toml`).

## Example

See `plugins/example-uppercase/` — a minimal uppercase plugin (WAT + compiled WASM, 160 B). Integration tests in `crates/hexforge-plugin-host` (`plugin_transform_uppercase_via_wasm`, `plugin_transform_reverse_via_wasm`) load it via `PluginTransform`.

```bash
cargo test -p hexforge-plugin-host -- plugin_transform_uppercase
```

