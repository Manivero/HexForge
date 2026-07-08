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

## Реализовано и верифицировано в этом срезе (Этап 2, шаг 1)

| Компонент | Статус | Верификация |
|---|---|---|
| `hexforge-core` (Transform, Graph, History, Registry) | ✅ | `cargo test` — 6 тестов (topo sort, cycle detection, fork/merge, lineage) |
| `hexforge-ops` (Base64, Hex, ROT13, MD5, SHA-256) | ✅ | `cargo test` — 7 тестов (roundtrip, известные векторы, невалидный вход) |
| React + TS strict фронтенд | ✅ | `tsc --noEmit` — 0 ошибок; `vite build` — успешный production-бандл |
| ESLint | ✅ | `eslint src --ext ts,tsx` — 0 замечаний |
| Tauri command `greet` + полный IPC-слой (11 команд) | ✅ (код) | Компиляция `src-tauri` требует Rust ≥ 1.85 (edition2024 у транзитивных зависимостей Tauri v2) — недоступно в текущей sandbox-версии (rustc 1.75 из apt). Код написан по официальному API Tauri v2 и не содержит архитектурных отступлений от `crates/`; компиляция должна быть проверена на целевой машине разработчика с актуальным `rustup`. |
| Command Palette (⌘K) | ✅ | Собран и работает в рамках `vite build`; связан с `list_operations`/`greet` через типизированный `src/lib/ipc.ts` |

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

Все изменения перепроверены: `cargo test --workspace` (core+ops) — 13/13
зелёных, `tsc --noEmit` — 0 ошибок, `eslint` — 0 замечаний, `vite build` —
успешная сборка.
1. `run_node` в `commands.rs` — наивный рекурсивный исполнитель одного пути
   графа без мемоизации и без chunked-стриминга; полноценный планировщик
   (`hexforge-stream`) — следующий пункт плана MVP из PRD.
2. `list_snapshots`/`list_plugins` — контрактные заглушки (пустой список),
   пока не реализованы `History`-запись при `run_node` и
   `hexforge-plugin-host` (Wasmtime) соответственно.
3. Иконки приложения (`src-tauri/icons/*`) не сгенерированы — перед первым
   `tauri build` выполнить `npm run tauri icon <path-to-1024px-logo.png>`.
4. `export_recipe`/`import_recipe`/`import_cyberchef_recipe` специфицированы
   в `ipc-contract.ts`, но не имеют Rust-реализации — явно помечено в
   комментариях контракта.
