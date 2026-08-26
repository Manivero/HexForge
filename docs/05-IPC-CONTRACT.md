# HexForge — Data Flow & IPC Contract

Единственный источник правды по контракту — файл `src/lib/ipc-contract.ts`
(копия ниже). Rust-сторона (`src-tauri/src/commands.rs`) обязана иметь
структуру `#[derive(Serialize/Deserialize)]`, побайтово соответствующую этим
типам; расхождение — баг, а не "особенность". CI-шаг `check-ipc-parity`
(см. `07` boilerplate) сверяет сгенерированный из Rust JSON Schema с этим
файлом при каждом билде.

## 1. Принципы контракта

1. Байты никогда не пересекают границу IPC напрямую для файлов >1МБ — только
   `SourceHandle` (opaque UUID). Просмотр данных — через `preview_bytes` с
   явным диапазоном (см. `03-INFORMATION-ARCHITECTURE`, §4).
2. Любая команда, способная выполняться дольше 16ms, обязана быть заведена
   как `async fn` на Rust-стороне и вызываться через `invoke` без `await`-блокировки
   рендера — прогресс идёт отдельным event-каналом (`listen('op://progress')`),
   не через возврат промиса.
3. Ошибки — всегда типизированный `Result<T, HexForgeError>`, никогда голая строка.

## 2. Полный контракт (TypeScript)

```typescript
// src/lib/ipc-contract.ts
// Единственный источник правды для типов IPC. Не редактировать вручную
// без синхронного изменения src-tauri/src/commands.rs.

// ---------- Базовые типы ----------

export type NodeId = string;   // UUID v4
export type SourceHandle = string; // UUID v4, непрозрачный хэндл на байтовый источник в Rust-памяти
export type SnapshotId = string;   // UUID v4
export type PluginId = string;

export type MemoryCost = "constant" | "per_chunk" | "full_buffer";

export interface TransformCapabilities {
  deterministic: boolean;
  streamable: boolean;
  memoryCost: MemoryCost;
}

export interface OperationDescriptor {
  id: string;                 // напр. "encoding.base64.decode"
  version: string;            // semver
  displayName: string;
  category: string;           // напр. "Encoding"
  paramsSchema: unknown;      // JSON Schema, рендерится в <OperationParamsForm>
  capabilities: TransformCapabilities;
}

export interface OperationNodeDto {
  id: NodeId;
  operationId: string;
  operationVersion: string;
  params: unknown;
  inputs: NodeId[];
}

export interface GraphDto {
  nodes: Record<NodeId, OperationNodeDto>;
}

export type HexForgeErrorKind =
  | "InvalidParameter"
  | "InvalidInput"
  | "MemoryBudgetExceeded"
  | "CycleDetected"
  | "DanglingInput"
  | "PluginSignatureInvalid"
  | "PluginCapabilityDenied"
  | "Cancelled"
  | "Internal";

export interface HexForgeError {
  kind: HexForgeErrorKind;
  message: string;
  field?: string;       // для InvalidParameter
  limitMb?: number;      // для MemoryBudgetExceeded
  nodeId?: NodeId;       // для CycleDetected/DanglingInput
}

export type HexForgeResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: HexForgeError };

// ---------- Commands: реестр операций ----------

/** invoke<OperationDescriptor[]>("list_operations") */
export type ListOperationsResponse = OperationDescriptor[];

// ---------- Commands: источники данных ----------

export interface OpenFileRequest {
  path: string;
}
export interface OpenFileResponse {
  handle: SourceHandle;
  sizeBytes: number;
  detectedMime?: string;
}
/** invoke<OpenFileResponse>("open_file", req: OpenFileRequest) */

export interface CreateLiteralSourceRequest {
  /** Небольшие литералы (напр. введённый вручную текст), лимит 16МБ на этом пути */
  utf8: string;
}
export interface CreateLiteralSourceResponse {
  handle: SourceHandle;
  sizeBytes: number;
}
/** invoke<CreateLiteralSourceResponse>("create_literal_source", req) */

export interface PreviewBytesRequest {
  handle: SourceHandle;
  offset: number;
  length: number;   // максимум 1MB за один запрос — обеспечивается сервером, не клиентом
}
export interface PreviewBytesResponse {
  /** base64 — единственный безопасный способ передать произвольные байты через JSON-IPC */
  base64Chunk: string;
  actualLength: number;
}
/** invoke<PreviewBytesResponse>("preview_bytes", req) */

export interface ReleaseSourceRequest {
  handle: SourceHandle;
}
/** invoke<void>("release_source", req) — явное освобождение памяти (FR "немедленная выгрузка") */

export interface PatchSourceRequest {
  handle: SourceHandle;
  offset: number;
  /** Перезаписываемые байты (base64). Только в границах текущего размера. */
  bytesBase64: string;
}
export interface PatchSourceResponse {
  newSizeBytes: number;
}
/** invoke<PatchSourceResponse>("patch_source", req) — точечная перезапись
 *  InMemory-источника (FR Hex Editor MVP): без роста, Mapped — read-only. */

// ---------- Commands: граф ----------

export interface SetGraphRequest {
  graph: GraphDto;
}
/** invoke<HexForgeResult<void>>("set_graph", req) — валидирует DAG (без циклов), инвалидирует downstream */

export interface RunNodeRequest {
  nodeId: NodeId;
  /** Если true — выполняется только сам узел, downstream не пересчитывается (превью, FR-1.6) */
  previewOnly: boolean;
}
export interface RunNodeResponse {
  outputHandle: SourceHandle;
  outputSizeBytes: number;
  durationMs: number;
}
/** invoke<HexForgeResult<RunNodeResponse>>("run_node", req)
 *  Долгие запуски репортят прогресс через event "op://progress" { nodeId, bytesProcessed, bytesTotal? }
 *  и поддерживают отмену через invoke("cancel_node", { nodeId }) */

export interface CancelNodeRequest {
  nodeId: NodeId;
}
/** invoke<boolean>("cancel_node", req) — кооперативная отмена: true, если
 *  активный запуск найден и флаг выставлен (one-shot); планировщик завершает
 *  цепочку ошибкой kind="Cancelled" на ближайшем чекпоинте. */

// ---------- Commands: history / time-travel ----------

export interface SnapshotDto {
  id: SnapshotId;
  parent: SnapshotId | null;
  nodeId: NodeId;
  operationId: string;
  operationVersion: string;
  params: unknown;
  inputContentHash: string;   // blake3 hex
  outputContentHash: string | null;
}

/** invoke<SnapshotDto[]>("list_snapshots") */
export type ListSnapshotsResponse = SnapshotDto[];

export interface JumpToSnapshotRequest {
  snapshotId: SnapshotId;
}
/** invoke<HexForgeResult<RunNodeResponse>>("jump_to_snapshot", req) — лениво пересчитывает, если output вытеснен из кэша */

// ---------- Commands: экспорт/импорт recipe ----------

export interface ExportRecipeRequest {
  graph: GraphDto;
  targetPath: string;
}
/** invoke<HexForgeResult<void>>("export_recipe", req) */

export interface ImportRecipeRequest {
  sourcePath: string;
}
export interface ImportRecipeResponse {
  graph: GraphDto;
  /** операции, которых нет в текущей версии реестра — UI обязан явно показать список (FR-4.2 воспроизводимость) */
  missingOperations: string[];
}
/** invoke<HexForgeResult<ImportRecipeResponse>>("import_recipe", req) */

export interface ImportCyberChefRecipeRequest {
  sourcePath: string;
}
export interface ImportCyberChefRecipeResponse {
  graph: GraphDto;
  unmappedOperations: { cyberChefId: string; reason: string }[];
}
/** invoke<HexForgeResult<ImportCyberChefRecipeResponse>>("import_cyberchef_recipe", req) */

// ---------- Commands: плагины ----------

export interface PluginManifestDto {
  id: PluginId;
  name: string;
  version: string;
  author: string;
  signatureValid: boolean;
  requestedCapabilities: PluginCapability[];
  grantedCapabilities: PluginCapability[];
}

export type PluginCapability = "filesystem_read" | "filesystem_write" | "network";

/** invoke<PluginManifestDto[]>("list_plugins") */
export type ListPluginsResponse = PluginManifestDto[];

export interface InstallPluginRequest {
  wasmPath: string;
  manifestPath: string;
}
/** invoke<HexForgeResult<PluginManifestDto>>("install_plugin", req) */

export interface GrantCapabilityRequest {
  pluginId: PluginId;
  capability: PluginCapability;
}
/** invoke<HexForgeResult<void>>("grant_capability", req) */

export interface RevokeCapabilityRequest {
  pluginId: PluginId;
  capability: PluginCapability;
}
/** invoke<HexForgeResult<void>>("revoke_capability", req) */

// ---------- Events (Rust -> Frontend, через @tauri-apps/api/event) ----------

export interface OpProgressEvent {
  nodeId: NodeId;
  bytesProcessed: number;
  bytesTotal: number | null;
}
/** event name: "op://progress" */

export interface OpMemoryWarningEvent {
  nodeId: NodeId;
  estimatedMb: number;
  /** UI обязан показать confirm-диалог перед продолжением (FR-5.3) */
}
/** event name: "op://memory-warning" */

export interface GraphInvalidatedEvent {
  staleNodeIds: NodeId[];
}
/** event name: "graph://invalidated" */
```

## 3. Диаграмма потока (пример: пользователь меняет параметр узла)

```
React (ParamField.onChange)
  → zustand graphSlice.updateNodeParams(nodeId, patch)      [синхронно, <1ms, UI откликается мгновенно]
  → debounced (120ms) invoke("set_graph", { graph })         [не блокирует рендер]
      → Rust: валидация DAG → graph::downstream_of(nodeId)
      → emit("graph://invalidated", { staleNodeIds })
  React (слушатель graph://invalidated)
    → помечает OperationNode.status = 'stale' для затронутых узлов [визуально, немедленно]
  Если nodeId был выделен и previewOnly:
    → invoke("run_node", { nodeId, previewOnly: true })
      → Rust: spawn_blocking / rayon, стриминг прогресса через op://progress
      → resolve → RunNodeResponse { outputHandle }
    React → invoke("preview_bytes", { handle: outputHandle, offset: 0, length: 65536 })
      → обновление <PreviewDock>
```

Ключевое свойство потока: путь "пользователь напечатал символ параметра" →
"UI обновился" никогда не ждёт Rust — оптимистичное обновление стейта
происходит локально, а реальный пересчёт и его результат приходят
асинхронно и лишь дополняют картину (статус узла, превью), что и даёт
целевые < 16ms из NFR-1.
