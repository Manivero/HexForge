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

## Статус реализации (Этап 2, срез 1)

- ✅ `hexforge-core` + `hexforge-ops`: компилируются, 13/13 unit-тестов зелёные
  (Base64, Hex, ROT13, MD5, SHA-256; topo-sort/cycle-detection/fork-merge графа)
- ✅ React/TS strict фронтенд: `tsc --noEmit` чисто, `vite build` собирается,
  ESLint без замечаний
- ✅ Command Palette (⌘K) — первый и центральный UI-компонент, подключён к
  живому реестру операций через типизированный IPC-слой
- ⏳ `src-tauri` (мост Rust↔WebView): написан по API Tauri v2, компиляция
  требует Rust ≥ 1.85 — см. ограничения sandbox-окружения в
  `docs/07-PROJECT-SCAFFOLD.md`

## Быстрый старт

```bash
npm install
npm run dev          # фронтенд отдельно, http://localhost:1420
# или, с установленным Rust ≥ 1.85:
npm run tauri dev    # полное нативное приложение
```

## Дальше по плану MVP (см. PRD §3)

`hexforge-stream` (chunked-планировщик, N-арные merge-узлы) →
`GraphCanvas`/`OperationNode` (визуализация DAG) → `HexViewer` (virtualized) →
`History`/Time-Travel запись при каждом `run_node` → `hexforge-plugin-host`
(Wasmtime sandbox).
