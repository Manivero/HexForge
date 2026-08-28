# HexForge

Native desktop tool for data transformation and analysis — a CyberChef successor built on **Tauri v2 + Rust** with a **React 18** frontend and a **Node Graph** instead of linear recipes.

## What is HexForge

HexForge is designed for security researchers, malware analysts, DFIR specialists, reverse engineers, and CTF players who need to decode, transform, and analyze arbitrary data — offline, natively, without browser limitations.

Unlike CyberChef's linear recipe list, HexForge uses a directed acyclic graph (DAG) where each node is an operation with multiple inputs/outputs. The Command Palette (⌘K) is the primary interface: every action, operation, and navigation is available through keyboard-driven search.

## Key Features

- **Node Graph workspace** — DAG-based data transformation pipeline
- **Command Palette (⌘K)** — fuzzy search across all operations and commands
- **Time-Travel History** — state DAG with jump-to-snapshot and branching
- **Hex Viewer/Editor** — paginated hex dump with byte patching
- **Parallel streaming pipeline** — chunked execution with backpressure and fusion
- **Content-addressed caching** — memoization by reproducibility key
- **CLI mode** — headless recipe runner for CI/scripting
- **Recipe export/import** — portable JSON format

## Operations

### Encoding
| Operation | Description |
|-----------|-------------|
| `base32.encode` / `base32.decode` | RFC 4648 Base32 |
| `base58.encode` / `base58.decode` | Bitcoin Base58 |
| `base64.encode` / `base64.decode` | Standard / URL-safe / Custom (64-char alphabet) Base64 — streamable (PerChunk) |
| `base85.encode` / `base85.decode` | Ascii85/Base85 (4→5, `z` for zeros) |
| `hex.encode` / `hex.decode` | Hexadecimal encoding |
| `quoted_printable.encode` / `decode` | Quoted-Printable (RFC 2045) |
| `json.pretty` / `json.minify` | JSON formatting |
| `xml.pretty` | XML pretty-print |
| `yaml.pretty` | YAML pretty-print |
| `msgpack.decode` | MessagePack to JSON |
| `auto_decode` | Magic Wand auto-detect (base64/hex) |
| `protobuf.decode_raw` | Raw Protobuf wire-format walk |

### Hashing
| Operation | Description |
|-----------|-------------|
| `blake3` | BLAKE3 cryptographic hash (256-bit) |
| `blake2b` | BLAKE2b-512 hash |
| `blake2s` | BLAKE2s-256 hash |
| `crc32` | IEEE CRC-32 checksum |
| `md5` | MD5 hash |
| `sha1` | SHA-1 hash |
| `sha256` | SHA-256 hash |
| `sha512` | SHA-512 hash |
| `sha3_256` | SHA3-256 hash |
| `ssdeep` | SSDEEP fuzzy hash |

### Cryptography
| Operation | Description |
|-----------|-------------|
| `rot_n` | ROT-N shift (0–25) |
| `xor` | Byte-wise XOR with cycling UTF-8 key |
| `rc4` | RC4 stream cipher (hexKey support) |
| `aes.encrypt` / `aes.decrypt` | AES-128/192/256 ECB/CBC (PKCS7) / CTR (no padding, hex key/iv) |
| `aes_gcm.encrypt` / `aes_gcm.decrypt` | AES-GCM AEAD (128/256, nonce/AAD) |
| `chacha20` | ChaCha20 stream cipher (hex key/nonce) |
| `chacha20_poly1305.encrypt` / `decrypt` | ChaCha20-Poly1305 AEAD |

### Network
| Operation | Description |
|-----------|-------------|
| `url_encode` | Percent-encoding (RFC 3986) |
| `url_decode` | Percent-decoding (+ → space) |
| `url_parse` | URL parse to JSON |
| `jwt_decode` | JWT header/payload decode (base64url) |
| `pcap_info` | PCAP global/packet header summary |
| `pcap_parse` | PCAP L2-L4 parse (Ethernet/IP/TCP) |
| `http_parse` | HTTP request/response parse |
| `user_agent_parse` | User-Agent browser/OS/device |
| `ip_parse` | IP parse (v4/v6, private/loopback) |
| `dns_parse` | DNS header/question parse |

### Text
| Operation | Description |
|-----------|-------------|
| `case_transform` | Upper / lower / title case |
| `html_encode` / `html_decode` | HTML entities (named + numeric) |
| `regex_extract` | Regex extract matches (one per line) |
| `regex_replace` | Regex replace with captures ($1) |
| `unicode_normalize` | Unicode NFC/NFD/NFKC/NFKD |
| `trim` | Trim whitespace (both/start/end) |
| `remove_whitespace` | Remove all whitespace (streamable) |
| `reverse` | Byte-level reversal |
| `rot13` | ROT13 substitution |

### Compression
| Operation | Description |
|-----------|-------------|
| `gzip.compress` / `gzip.decompress` | Gzip (RFC 1952) via flate2 |
| `zlib.compress` / `zlib.decompress` | Zlib (RFC 1950) via flate2 |
| `deflate.compress` / `deflate.decompress` | Raw Deflate (RFC 1951) via flate2 |
| `bzip2.compress` / `bzip2.decompress` | Bzip2 via bzip2 crate |
| `lzma.compress` / `lzma.decompress` | LZMA/XZ via xz2 |

### Streaming
| Operation | Description |
|-----------|-------------|
| `concat` | N-ary input concatenation (MergeTransform) |
| `diff` | Byte-level diff of 2 inputs (MergeTransform) |

### Binary Analysis
| Operation | Description |
|-----------|-------------|
| `strings_extract` | Printable ASCII/UTF-16LE/BE sequences (`strings(1)` + UTF-16) |
| `entropy` | Shannon entropy (0–8 bits/byte) |
| `elf_info` | ELF header parse (goblin) |
| `pe_info` | PE header parse (goblin) |
| `macho_info` | Mach-O header parse (goblin) |
| `magic` | Magic bytes detect via infer |

## Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  React 18   │◄───►│   Tauri v2 IPC    │◄───►│  Rust Engine     │
│  Frontend   │     │   (typed JSON)    │     │                  │
└─────────────┘     └──────────────────┘     └────────┬─────────┘
                                                       │
                                              ┌────────▼────────┐
                                              │ hexforge-engine  │
                                              │ scheduler/cache  │
                                              └────────┬────────┘
                                                       │
                     ┌──────────┬─────────────────────┼──────────┐
                     │          │                     │          │
                hexforge-  hexforge-            hexforge-  hexforge-
                core       ops                 stream     cli
```

| Crate | Purpose |
|-------|---------|
| `hexforge-core` | Domain model: `Transform` trait, DAG graph, snapshots. Zero I/O. |
| `hexforge-ops` | Built-in operations implementing `Transform` via `inventory` (196 tests) |
| `hexforge-stream` | Chunked I/O primitives (pure, no domain knowledge) — 64 MiB |
| `hexforge-engine` | Execution engine: scheduler, cache (true LRU), cancellation, history, diff |
| `hexforge-plugin-host` | Ed25519 manifest verify + Wasmtime fuel metering (NFR-9) + capability sandbox |
| `src-tauri` | Tauri shell: typed IPC commands, frontend serving |
| `hexforge-cli` | Headless recipe runner (no GUI) |

Key design decisions:
- **`Transform` trait** — single contract for all operations (sync, pure function)
- **`MergeTransform` trait** — N-ary operations (e.g., concat)
- **`inventory` crate** — compile-time registration, no central op list to maintain
- **Fusion pipeline** — adjacent streamable nodes merged into one chunk loop; parallel stages via bounded channels
- **Content-addressed LRU cache** — memoization keyed by `(op@version, input_hash, params)`
- **Cooperative cancellation** — token-based, checked at chunk boundaries

## Getting Started

### Prerequisites

- Node.js ≥ 18
- Rust ≥ 1.85 (via rustup)

### Development

```bash
npm install
npm run dev          # Frontend only → http://localhost:1420
npm run tauri dev    # Full native app (requires Rust)
```

### Production build

```bash
npm run tauri build
```

### CLI

```bash
cargo run -p hexforge-cli -- run recipe.hexforge --in input.bin --out output.bin
cargo run -p hexforge-cli -- validate recipe.hexforge
```

## Testing

```bash
# Rust tests (all crates)
cargo test --workspace

# Static analysis
cargo clippy --workspace --all-targets

# Benchmarks (NFR-1)
cargo bench -p hexforge-stream -- --verbose

# Frontend lint + type check + build
npm run lint
npm run test:fe     # Unit tests (node:test, zero deps)
npm run build       # tsc --noEmit + vite build
```

## Roadmap

| Priority | Item |
|----------|------|
| Done | Native performance profiling — `cargo bench -p hexforge-stream` criterion |
| Done | i18n en/ru — `src/lib/i18n.ts` + locale toggle |
| Done | Plugin host — `hexforge-plugin-host` Ed25519 verify + Wasmtime fuel metering (NFR-9) + capability sandbox |
| Done | Import CyberChef recipes — `import_cyberchef_recipe` |
| Future | Magic Wand — heuristic chain detection (auto_decode implemented) |
| Done | Diff between snapshots (FR-4.3) — `diff_snapshots` |
| Done | Crypto AEAD — AES-GCM (128/256) + ChaCha20-Poly1305 |
| Deferred | Real-time collaboration, cloud sync, mobile clients |

See [PRD §3](docs/01-PRD.md) for full requirements and [docs/07](docs/07-PROJECT-SCAFFOLD.md) for current implementation status.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Ensure all checks pass:
    ```bash
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo bench -p hexforge-stream --no-run
    npm run lint && npm run test:fe && npm run build
    ```
4. Commit with a descriptive message following [Conventional Commits](https://www.conventionalcommits.org/)
5. Open a pull request

## Security

- **No network access** — fully offline; WebView has no direct fs/network permissions
- **CSP enforced** — `default-src 'self'` in production builds
- **Typed IPC** — all Tauri commands validated against TS contract; golden tests prevent drift
- **No secrets in code** — environment variables excluded via `.gitignore`

Report vulnerabilities privately to the maintainers. Do not open public issues for security concerns.

## License

[MIT](LICENSE)

## Known Limitations

- **Compression fully implemented** — gzip/zlib/deflate/bzip2/lzma done
- **Plugin host — Wasmtime implemented** — `hexforge-plugin-host` Ed25519 verify + fuel-metered Wasmtime (NFR-9) + capability sandbox; Component Model WIT next
- **Crypto AES modes** — ECB/CBC/CTR (PKCS7 for ECB/CBC, no padding for CTR), GCM and ChaCha20-Poly1305 done
- **DNS parser hardened** — `network.dns_parse` with compression pointers, loop guards, 8 tests
- **Base64 streamable** — `encoding.base64` now PerChunk (encode leftover 2 bytes, decode leftover 3 chars) for 32GB files (NFR-2)
- **Base85** — `encoding.base85` Ascii85 (4→5, `z` for 4 zeros) with 5 tests
- **Mapped sources copy-on-write** — file-backed sources patch via COW to InMemory (FR Hex Editor)
- **previewOnly downstream warming** — `previewOnly=false` now warms downstream cache concurrently via blocking pool (fork graphs parallel)
- **No multi-source graphs** — recipes assume exactly one root source node (CLI supports multi-root via same input)
- **Parallel streaming pipeline** — fusion + bounded channels (4×64 MiB = 256 MiB per stage) implemented per FR-5.2
- **64 MB chunks implemented** — `DEFAULT_CHUNK_SIZE_BYTES = 64 MiB` for file I/O and `apply_chunk`
- **i18n ready** — `src/lib/i18n.ts` en/ru, locale toggle in header (App.tsx:22)
