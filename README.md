# HexForge

VS Code для анализа и трансформации данных. Нативный преемник CyberChef:
Tauri v2 + Rust ядро + React 18/TS strict фронтенд, Node Graph вместо
линейного recipe, Command Palette (⌘K) как первичный интерфейс.

## Документация (Этап 1 — проектирование)

1. [`docs/01-PRD.md`](docs/01-PRD.md) — функциональные и нефункциональные требования
2. [`docs/02-COMPETITIVE-GAP-ANALYSIS.md`](docs/02-COMPETITIVE-GAP-ANALYSIS.md) — HexForge vs CyberChef
3. [`docs/03-INFORMATION-ARCHITECTURE.md`](docs/03-INFORMATION-ARCHITECTURE.md) — дерево UI-компонентов
4. [`docs/04-RUST-CORE-ARCHITECTURE.md`](docs/04-RUST-CORE-ARCHITECTURE.md) — Cargo workspace, трейт `Transform`
5. [`docs/05-IPC-CONTRACT.md`](docs/05-IPC-CONTRACT.md) — полный TS-контракт Tauri commands
6. [`docs/06-DESIGN-SYSTEM.md`](docs/06-DESIGN-SYSTEM.md) — design tokens (см. `tailwind.config.ts`)
7. [`docs/07-PROJECT-SCAFFOLD.md`](docs/07-PROJECT-SCAFFOLD.md) — структура проекта + статус верификации

## Статус реализации (Этап 2)

- ✅ `hexforge-core` + `hexforge-ops`: компилируются, unit-тесты зелёные
  (Base64, Hex, ROT13, MD5, SHA-256; topo-sort/cycle-detection/fork-merge графа;
  lineage-обход истории с защитой от parent-циклов)
- ✅ React/TS strict фронтенд: `tsc --noEmit` чисто, `vite build` собирается,
  ESLint без замечаний
- ✅ Command Palette (⌘K) — первичный интерфейс, подключён к живому реестру
  операций через типизированный IPC-слой
- ✅ `hexforge-stream` MVP + `hexforge-engine`: chunked `apply_chunk`
  для streamable-операций, memoization (content-addressed LRU, 256MB),
  кооперативная отмена (`cancel_node` → `Cancelled`), merge-узлы через
  `MergeTransform` + `streaming.concat`; движок вынесен из GUI-шелла в
  крейт `hexforge-engine`, чанк-примитивы — в крейт `hexforge-stream`
- ✅ Time-Travel (FR-4): `jump_to_snapshot` — lineage-реплей из корневого
  источника с верификацией content-hash'ей и переносом головы истории;
  HistoryPanel — дерево по parent-ссылкам с маркерами ветвления,
  клик = прыжок; кнопка Cancel активирует кооперативную отмену из UI
- ✅ Hex Editor MVP (FR §3): `patch_source` — точечная перезапись байтов
  InMemory-источника в границах (Mapped read-only); патч байта/региона
  (hex-пары) в HEX-режиме PreviewDock с автоперезагрузкой страницы
- ✅ `hexforge-cli` (FR-7.3): `hexforge-cli run recipe.hexforge --in file
  --out file` на том же движке без GUI; формат рецепта = GraphDto JSON
- ✅ Первый сквозной поток данных (05-IPC §3): InputPanel (литеральный
  источник) → debounced `set_graph` (120ms) → `Run node` → GraphCanvas
  (вертикальный рельс DAG с выбором узлов) → PreviewDock — TEXT ≤4KB и постраничный
  HexViewer (◀▶/переход по смещению, ASCII), stale-бейдж,
  кнопка Cancel во время запуска; InspectorPanel —
  авто-форма параметров из JSON Schema (FR-3.2); статус-бар со счётчиком
  снапшотов; без нативного бэкенда UI деградирует мягко (vite dev)
- ✅ `src-tauri` (мост Rust↔WebView): workspace собирается целиком
  (`cargo build --workspace`), иконки сгенерированы (`src-tauri/icons/`),
  `run_node` — async (spawn_blocking) со стримингом `op://progress`, пишет
  Snapshot в Time-Travel History (blake3 content-hash'и); `list_snapshots`,
  `export_recipe`/`import_recipe` реализованы; паритет IPC-типов защищён
  golden-тестами; юнит-тесты командного слоя — зелёные
- ⏳ Заглушки контракта: `list_plugins` (ждёт `hexforge-plugin-host`),
  `import_cyberchef_recipe`

## Быстрый старт

```bash
npm install
npm run dev          # фронтенд отдельно, http://localhost:1420
# или, с установленным Rust ≥ 1.85:
npm run tauri dev    # полное нативное приложение
```

Проверки: `npm run lint`, `npm run test:fe`, `npm run build`, `cargo test --workspace`.

CLI-режим (FR-7.3):

```bash
cargo run -p hexforge-cli -- run recipe.hexforge --in input.bin --out output.bin
```

## Дальше по плану MVP (см. PRD §3)


`hexforge-plugin-host`
(Wasmtime sandbox).
