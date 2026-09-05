# HexForge Plugin SDK — minimal WIT component template

Template for a third-party `hexforge:plugin@0.1.0` component (`world hexforge-plugin`,
interface `transform`). Copy this directory out of the repo and adapt.

Files:

- `wit/plugin.wit` — the exact WIT contract the host implements
  (copy of `crates/hexforge-plugin-host/wit/plugin.wit`).
- `src/lib.rs` — minimal `wit-bindgen` implementation (ASCII uppercase).
- `manifest.json` — plugin identity + capabilities for the install request.
- `Cargo.toml` — standalone crate (`cdylib`), detached from the HexForge workspace.

## Full lifecycle: create → build → sign → manifest → install → grant → execute

Prerequisites: Rust with the `wasm32-wasip1` target, `wasm-tools`, and a `hexforge-cli`
binary from this repo.

```bash
# 0. Copy the template
cp -r plugins/example-wit ~/my-plugin && cd ~/my-plugin

# 1. Create: edit src/lib.rs (get-id must be unique in the registry),
#    manifest.json (id/name/version/author), wit/plugin.wit stays untouched.

# 2. Build: core module, then wrap as a WIT component
cargo build --release --target wasm32-wasip1
wasm-tools component new target/wasm32-wasip1/release/hexforge_example_wit.wasm \
  -o plugin.wasm

# 3. Validate the manifest early (CI-friendly, fails before signing)
hexforge-cli plugin validate manifest.json
# OK: manifest valid: id=example.wit-uppercase version=1.0.0

# 4. Keygen (once) + sign EXACTLY the bytes you ship
hexforge-cli plugin keygen
# pubkey=<64 hex>
# signing_key=<64 hex>   # keep secret
hexforge-cli plugin sign manifest.json --key <signing_key>
# signature=<128 hex>

# 5. Install: HexForge app → Plugins → Install, with
#    plugin.wasm + manifest.json + signature + pubkey.
#    The host verifies the Ed25519 signature (TOFU), parses + validates the
#    manifest, and load-checks the binary as component first, core module second
#    (legacy core-module plugins keep working via the manifest fallback).

# 6. Capability grant: privileged caps (filesystem_read, filesystem_write,
#    network) requested in manifest.json must be granted before install
#    succeeds — otherwise install fails with `capability denied`.

# 7. Execute: the plugin appears as a normal operation node; the host runs
#    `apply` under fuel (10M default) + memory (256 MiB) limits.
```

## Developer errors (what they mean)

| Error | Cause | Fix |
|---|---|---|
| `invalid manifest: field 'version' must be numeric "major.minor.patch"` | bad manifest semantics | fix `manifest.json`, re-validate |
| `manifest parse failed` / `is not a valid manifest file` | not JSON / wrong shape | fix JSON against the template |
| `invalid signature: signature verification failed` | bytes differ from signed, or wrong key | sign exactly the shipped bytes with the matching key |
| `not a component: file does not export the WIT world 'hexforge-plugin'` | legacy core module — NOT fatal | host falls back to manifest metadata + core ABI |
| `component is missing export '…' (unsupported WIT interface: expected …@…)` | component built for another WIT world/version | rebuild against `wit/plugin.wit` from this repo |
| `capability denied: 'network' requested but not granted` | privileged cap w/o grant | grant it, or drop from `requested_capabilities` |
| `fuel exhausted …` | infinite loop / too-heavy compute | fix the plugin; host default is 10M fuel units |
| `transform output exceeds the 10485760 byte limit` | plugin returned > 10 MiB | chunk the work; host-side OOM guard, not negotiable |
| `wasm trap in '…'` | plugin trapped (OOB, unreachable, …) | debug the plugin; host stays up |

## Versioning / ABI compatibility

- WIT contract identity is `hexforge:plugin@0.1.0` (host constants
  `WIT_PACKAGE`/`WIT_VERSION` in `hexforge-plugin-host`). Breaking WIT changes
  bump it; the host reports unknown contracts explicitly.
- Manifest `version` is the PLUGIN version (`major.minor.patch`, numeric);
  it is unrelated to the WIT version.
- Within one WIT version the host keeps backward compatibility: core-module
  plugins without any WIT exports keep working through the manifest fallback.
