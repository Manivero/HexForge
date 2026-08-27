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
| `hexforge-ops` — 42 операции | ✅ | `cargo test -p hexforge-ops` — 142 теста (см. ниже); `clippy -D warnings` 0 |
| Encoding | ✅ | base32 RFC4648, base58 Bitcoin, base64 std/url_safe, hex (streamable), quoted_printable RFC2045, json pretty/minify, xml pretty (quick-xml), msgpack (rmp-serde), protobuf raw walk — roundtrip + invalid input |
| Hashing | ✅ | blake3, blake2b/s, crc32 IEEE (cbf43926), md5, sha1/sha256/sha512, sha3_256 — known vectors |
| Compression | ✅ | gzip/zlib/deflate (flate2), bzip2 (bzip2), lzma/xz (xz2) — roundtrip + level param, large 50k |
| Text | ✅ | case_transform, html encode/decode, regex_extract/replace (regex 1), reverse, rot13, unicode_normalize (nfc/nfd/nfkc/nfkd) |
| Network | ✅ | url_encode/decode RFC3986, url_parse (url crate), jwt_decode (base64url), pcap_info (manual pcap header) |
| Crypto | ✅ | rot_n, xor (UTF-8 key), rc4 (hexKey), aes-128/192/256 ecb/cbc PKCS7 (aes+cbc+ecb), chacha20 (32B key/12B nonce) — NIST ECB vector, involution |
| Streaming | ✅ | concat (MergeTransform), diff (2-input byte diff) |
| Binary Analysis | ✅ | strings_extract, entropy, elf_info/pe_info (goblin), magic (infer) — reject non-elf/pe, PNG mime |
| Auto/Magic Wand | ✅ | encoding.auto_decode (base64/hex heuristic) |
| React + TS strict | ✅ | `tsc --noEmit` 0, `vite build` 252kB gzip 81kB, `eslint --max-warnings 0` 0 |
| i18n en/ru | ✅ | `src/lib/i18n.ts` 11 ключей, `useAppStore.locale` + toggle в `App.tsx:34`, `t(locale,key)` |
| Tauri IPC (15 команд) | ✅ | `cargo test -p hexforge` 22 golden-теста (wire format, sort_for_palette, graphDto, export/import, cancel, progress, invalidated) |
| Command Palette (⌘K) | ✅ | `vite build` + `fuzzyMatch.ts` 40 FE-тестов |
| Time-Travel | ✅ | `run_node` пишет Snapshot per node, `list_snapshots` ordered, `jump_to_snapshot` replay+fork, `diff_snapshots` FR-4.3 unified diff |
| Graph | ✅ | `removeNode` bridge + `clearGraph`, `compute_invalidated` downstream, `graphWalk` BFS + cycle guard — FE tests 40 |
| PreviewDock | ✅ | HexViewer 4KB pages, patch_source InMemory (Mapped RO), `preview_bytes` base64Chunk |
| Data-flow UI | ✅ | InputPanel → create_literal_source (16MB limit) → debounced set_graph → run_node → preview_bytes → PreviewDock |
| GraphCanvas | ✅ | BFS layout, selection, sourceHandle marker |
| InspectorPanel | ✅ | Auto-form from paramsSchema, stale badge FR-1.6 |
| hexforge-stream + engine | ✅ | `DEFAULT_CHUNK_SIZE_BYTES = 64 MiB` FR-5.2, fusion + parallel pipeline (stages×4×64MiB), `reproducibility_key` LRU (true LRU, 256MB), `cancel_node` Cancelled, `MergeTransform` |
| CLI | ✅ | `hexforge-cli run/validate` — `validate_graph` + DAG + version check, 4 tests + progress eprintln |
| Import | ✅ | `import_recipe` + `import_cyberchef_recipe` (To Base64/From Base64/To Hex/ROT13/XOR/URL/Gzip/Zlib mapping, unmapped list) |
| .env.example + .gitignore | ✅ | `*.out` + `!.env.example`, `.env` ignored, secrets scan 0 |
| Chunk 64 MiB (FR-5.2) | ✅ | `hexforge-stream:23` 64 MiB, parallel threshold = 64 MiB, engine tests with 64M vectors (24s) |

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

Все изменения перепроверены: `cargo test --workspace` — 217 зелёных (Rust: core 9, ops 142, stream 7, engine 33, tauri 22, cli 4) + 40 FE (fuzz, bytes, graph, i18n) — `cargo clippy -D warnings` 0, `tsc --noEmit` 0, `eslint` 0, `vite build` 252kB, `npm run test:fe` 40.
1. Планировщик (hexforge-engine): chunked `apply_chunk` + FUSION + параллельный конвейер (stages×4×64MiB=256MiB, FR-5.2), true LRU 256MB, `reproducibility_key`, `cancel_node` Cancelled, `MergeTransform` (concat/diff), `diff_snapshots` FR-4.3 unified diff, `import_cyberchef_recipe` mapping 14 ops, i18n en/ru.
2. Time-Travel: `jump_to_snapshot` replay + fork DAG + `diff_snapshots` byte/line diff; HistoryPanel DFS, `previewOnly` warming downstream; `compute_invalidated` downstream.
3. Иконки (`src-tauri/icons/*`) via `npx tauri icon`; смена брендинга — новый ≥1024×1024.
4. CI (`.github/workflows/ci.yml`): frontend (lint+build) + rust (cargo test --workspace) на Windows; `npm ci` + `npm audit` 0 vulns, `cargo audit` — следующий.
5. UI: linear chain + GraphCanvas BFS + Inspector auto-form + PreviewDock 4KB pages + patch_source; `i18n.ts` locale toggle; `package-lock.json` committed.
