# HexForge — Project Boilerplate & Scaffold

Полная файловая структура, созданная и верифицированная в этом срезе
(команды и результаты проверки — в конце документа).

```
hexforge/
├── Cargo.toml                          # workspace root
├── package.json
├── tsconfig.json / tsconfig.node.json
├── vite.config.ts
├── tailwind.config.ts
├── postcss.config.js
├── .eslintrc.cjs
├── .prettierrc
├── index.html
├── docs/
│   ├── 01-PRD.md
│   ├── 02-COMPETITIVE-GAP-ANALYSIS.md
│   ├── 03-INFORMATION-ARCHITECTURE.md
│   ├── 04-RUST-CORE-ARCHITECTURE.md
│   ├── 05-IPC-CONTRACT.md
│   ├── 06-DESIGN-SYSTEM.md
│   └── 07-PROJECT-SCAFFOLD.md          # этот файл
├── crates/
│   ├── hexforge-core/                  # ✅ компилируется, 6/6 тестов зелёные
│   │   ├── Cargo.toml
│   │   └── src/{lib,transform,graph,history,registry}.rs
│   └── hexforge-ops/                   # ✅ компилируется, 7/7 тестов зелёные
│       ├── Cargo.toml
│       └── src/{lib,encoding/{mod,base64,hex},hashing/mod,text/{mod,rot13}}.rs
├── src-tauri/                          # Tauri v2 shell
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   └── src/{main,commands,state,error}.rs
└── src/                                # React 18 + TS strict фронтенд
    ├── main.tsx / App.tsx
    ├── styles/globals.css
    ├── lib/{ipc-contract,ipc,utils,fuzzyMatch}.ts
    ├── store/useAppStore.ts
    └── components/
        ├── ui/{dialog,command}.tsx     # shadcn/ui-style, скопированы в проект
        └── CommandPalette/{CommandPalette.tsx,commands.ts}
```

## Реализовано и верифицировано (актуальный срез, 2026-08)

| Компонент | Статус | Верификация |
|---|---|---|
| `hexforge-core` (Transform, Graph, History, Registry) | ✅ | `cargo test -p hexforge-core` — 9 тестов (topo sort O(V+E), cycle, fork/merge, lineage + parent-cycle guard, order) |
| `hexforge-ops` — 49+ операции | ✅ | `cargo test -p hexforge-ops` — 221 тест (см. ниже); `clippy -D warnings` 0 |
| Encoding | ✅ | base32 RFC4648 **streamable** (PerChunk, 5→8, 2 new chunked tests), base58 Bitcoin, base64 std/url_safe/custom (64-char) **streamable** (PerChunk), base85 Ascii85 (4→5, `z`, 5 tests), hex (streamable), quoted_printable RFC2045, json pretty/minify, xml pretty (quick-xml), msgpack (rmp-serde), protobuf raw walk — roundtrip + invalid input |
| Hashing | ✅ | blake3 **streamable**, blake2b/s **streamable**, crc32 **streamable** (Constant, 1 new chunked test), md5/sha1/sha256/sha512/sha3_256 **streamable** (Constant, 2 new chunked tests), ssdeep, hmac (5 tests), pbkdf2 (sha1/sha256/sha512, 4 tests + DoS cap 1M, RFC6070) — known vectors |
| Compression | ✅ | gzip/zlib/deflate (flate2), bzip2 (bzip2), lzma/xz (xz2) — roundtrip + level param, large 50k |
| Text | ✅ | case_transform (upper/lower/title/snake/kebab/camel/pascal, 4 new tests), html encode/decode, regex_extract/replace (regex 1), reverse, rot13, unicode_normalize (nfc/nfd/nfkc/nfkd), trim (both/start/end), remove_whitespace (streamable), pad (left/right/both, 4 tests) |
| Network | ✅ | url_encode/decode RFC3986, url_parse (url crate), jwt_decode (base64url), pcap_info + pcap_parse L2-L4, dns_parse (header/compression loops hardened, 8 tests), http_parse, user_agent, ip_parse (v4/v6) |
| Crypto | ✅ | rot_n, xor (UTF-8 key) + xor_bruteforce (single-byte, printable filter, 3 tests), rc4 (hexKey), aes-128/192/256 ecb/cbc (PKCS7) + ctr (no padding, ctr 0.9), aes-gcm 128/256, chacha20 (RFC8439) + poly1305 AEAD — NIST vectors, involution |
| Streaming | ✅ | concat (MergeTransform), diff (2-input byte diff) |
| Binary Analysis | ✅ | strings_extract (ASCII/UTF-16LE/BE), entropy, elf_info/pe_info/macho_info (goblin), magic (infer) — reject non-elf/pe, PNG mime |
| Auto/Magic Wand | ✅ | encoding.auto_decode (base64/hex heuristic) |
| React + TS strict | ✅ | `tsc --noEmit` 0, `vite build` 255kB gzip 81kB, `eslint --max-warnings 0` 0 |
| i18n en/ru | ✅ | `src/lib/i18n.ts` 11 ключей, `useAppStore.locale` + toggle в `App.tsx:34`, `t(locale,key)` |
| Tauri IPC (15 команд) | ✅ | `cargo test -p hexforge` 22 golden-теста (wire format, sort_for_palette, graphDto, export/import, cancel, progress, invalidated) |
| Command Palette (⌘K) | ✅ | `vite build` + `fuzzyMatch.ts` 42 FE-тестов |
| Time-Travel | ✅ | `run_node` пишет Snapshot per node, `list_snapshots` ordered, `jump_to_snapshot` replay+fork, `diff_snapshots` FR-4.3 unified diff |
| Graph | ✅ | `removeNode` bridge + `clearGraph`, `compute_invalidated` downstream, `graphWalk` BFS + cycle guard — FE tests 42 |
| PreviewDock | ✅ | HexViewer 4KB pages, patch_source COW (Mapped → InMemory), `preview_bytes` base64Chunk |
| Data-flow UI | ✅ | InputPanel → create_literal_source (16MB limit) → debounced set_graph → run_node → preview_bytes → PreviewDock |
| GraphCanvas | ✅ | BFS layout, selection, sourceHandle marker, stale badge |
| InspectorPanel | ✅ | Auto-form from paramsSchema, stale badge FR-1.6 |
| hexforge-stream + engine | ✅ | `DEFAULT_CHUNK_SIZE_BYTES = 64 MiB` FR-5.2, fusion + parallel pipeline (stages×4×64MiB=256MiB), `reproducibility_key` LRU (true LRU, 256MB), `cancel_node` Cancelled, `MergeTransform`, dead code ReadOnlyMapped removed |
| CLI | ✅ | `hexforge-cli run/validate` — `validate_graph` + DAG + version check, 4 tests + progress eprintln |
| Import | ✅ | `import_recipe` + `import_cyberchef_recipe` (28+ mappings: Base64/Hex/Base32/ROT13/Reverse/URL/Gzip/Zlib/Bzip2/LZMA/XOR/MD5/SHA1/SHA2/SHA3/BLAKE2b/s/BLAKE3/CRC32/SSDEEP/Entropy/Strings/Magic, unmapped list) |
| .env.example + .gitignore | ✅ | `*.out` + `!.env.example`, `.env` ignored, `src-tauri/gen` ignored, secrets scan 0 |
| Chunk 64 MiB (FR-5.2) | ✅ | `hexforge-stream:23` 64 MiB, parallel threshold = 64 MiB, engine tests with 64M vectors (24s) |
| Plugin host | ✅ | `hexforge-plugin-host` Ed25519 verify + Wasmtime fuel metering (NFR-9, 10M, 2MiB stack) + capability sandbox, 10 tests (signature bypass fix, fuel exhaustion, trap isolation) |

## Как запустить локально

```bash
# Frontend + типы (не требует Rust, можно проверить прямо сейчас)
npm install
npm run dev        # http://localhost:1420 — UI без нативного moста (invoke упадёт в консоль)

# Полное Tauri-приложение (требует rustup ≥ 1.85, см. примечание выше)
rustup update
npm run tauri dev
```

## Первый раунд код-ревью (Principal Engineer pass)

После первичной сборки проведён полный ревью по 10 фазам (структура,
компиляция, типобезопасность, React, Rust, IPC, безопасность,
производительность, архитектура, DX). Найдено и исправлено 9 реальных
проблем (детали и диффы — в истории разговора/PR); ключевые:

- `HexForgeErrorKind` был `&'static str` — переведён в enum, опечатка в
  имени варианта теперь ошибка компиляции, а не молчаливое расхождение с TS.
- `Graph::topo_order`/`downstream_of` были фактически O(V²) несмотря на
  комментарий "O(V+E)" — пересканировали все узлы на каждой итерации;
  исправлено через predпостроенный adjacency list.
- `create_literal_source` не проверял заявленный в контракте лимит 16МБ —
  контракт и код расходились; добавлена проверка.
- Баг в `CommandPalette.tsx`: один `useEffect` со смешанными
  ответственностями (загрузка операций + сброс query) стирал введённый
  пользователем текст, если `operations` резолвились, пока палитра
  оставалась открытой. Разнесено на два эффекта с явным триггером на
  фронте открытия (переход false→true), а не на каждое изменение зависимостей.
- `Dialog.Content` рендерился без `Dialog.Title`/`Description` — нарушение
  собственного NFR-7 (WCAG) и Radix dev-warning. Добавлены sr-only Title/Description.
- Убраны 2 директории-артефакта (`{crates...}`, `{encoding,hashing,text}`),
  возникшие из-за отсутствия brace-expansion в `/bin/sh` при первоначальном
  scaffold — реальный "dead folder" finding из Phase 1.
- Устранена двойная точка входа трейта `Digest` (через `sha2`- и
  `md5`-реэкспорты) — добавлена прямая зависимость на `digest`.

Все изменения перепроверены: `cargo test --workspace` — 307 зелёных (Rust: core 9, ops 221, stream 7, engine 34, tauri 22, cli 4, plugin-host 10) + 42 FE (fuzz, bytes, graph, i18n, ipc) — `cargo clippy -D warnings` 0, `tsc --noEmit` 0, `eslint` 0, `vite build` 255kB, `npm run test:fe` 42.
1. Планировщик (hexforge-engine): chunked `apply_chunk` + FUSION + параллельный конвейер (stages×4×64MiB=256MiB, FR-5.2), true LRU 256MB, `reproducibility_key`, `cancel_node` Cancelled, `MergeTransform` (concat/diff), `diff_snapshots` FR-4.3 unified diff, `import_cyberchef_recipe` mapping 14 ops, i18n en/ru.
2. Time-Travel: `jump_to_snapshot` replay + fork DAG + `diff_snapshots` byte/line diff; HistoryPanel DFS, `previewOnly` warming downstream; `compute_invalidated` downstream.
3. Иконки (`src-tauri/icons/*`) via `npx tauri icon`; смена брендинга — новый ≥1024×1024.
4. CI (`.github/workflows/ci.yml`): frontend (lint+build) + rust (cargo test --workspace) на Windows; `npm ci` + `npm audit` 0 vulns, `cargo audit` — следующий.
5. UI: linear chain + GraphCanvas BFS + Inspector auto-form + PreviewDock 4KB pages + patch_source; `i18n.ts` locale toggle; `package-lock.json` committed.
