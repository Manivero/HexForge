// Единственный источник правды для типов IPC между React и Rust-ядром.
// См. docs/05-IPC-CONTRACT.md. Rust-сторона (src-tauri/src/commands.rs)
// обязана иметь структуры, побайтово соответствующие этим типам через
// #[serde(rename_all = "camelCase")].
//
// Статус реализации на Rust-стороне на срезе hexforge-stream (см. commands.rs
// и src-tauri/src/scheduler.rs):
//   ✅ реализовано:  greet, listOperations, openFile, createLiteralSource,
//                    previewBytes, releaseSource, setGraph,
//                    runNode (async + spawn_blocking; chunked apply_chunk для
//                    streamable-операций; memoization по reproducibility_key;
//                    merge-узлы через MergeTransform/streaming.concat;
//                    события op://progress; Snapshot в History за каждый узел),
//                    cancelNode (кооперативная отмена, kind="Cancelled"),
//                    listSnapshots, exportRecipe, importRecipe,
//                    listPlugins (заглушка до hexforge-plugin-host)
//   ⏳ специфицировано, не подключено: jumpToSnapshot,
//                    importCyberChefRecipe, installPlugin, grantCapability,
//                    revokeCapability — ждут Time-Travel UI /
//                    hexforge-plugin-host.
//
// Паритет типов с Rust-стороной защищён golden-тестами в
// src-tauri/src/commands.rs (tests::*_matches_ts_contract): переименование
// поля или смена регистра роняет cargo test до попадания в рантайм.
// Типы ниже описывают полный целевой контракт, не только реализованный срез.

export type NodeId = string; // UUID v4
export type SourceHandle = string; // UUID v4, непрозрачный хэндл на байтовый источник в Rust-памяти
export type SnapshotId = string; // UUID v4
export type PluginId = string;

export type MemoryCost = "constant" | "per_chunk" | "full_buffer";

export interface TransformCapabilities {
  deterministic: boolean;
  streamable: boolean;
  memoryCost: MemoryCost;
}

export interface OperationDescriptor {
  id: string;
  version: string;
  displayName: string;
  category: string;
  paramsSchema: unknown;
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
  field?: string;
  limitMb?: number;
  nodeId?: NodeId;
}

/** Rust-команды, возвращающие Result<T, HexForgeError>, кидают через invoke()
 * reject с этим объектом (Tauri сериализует Err-ветку как reject reason). */
export class HexForgeCommandError extends Error {
  constructor(public readonly details: HexForgeError) {
    super(details.message);
    this.name = "HexForgeCommandError";
  }
}

// ---------- open_file ----------
export interface OpenFileRequest {
  path: string;
}
export interface OpenFileResponse {
  handle: SourceHandle;
  sizeBytes: number;
  detectedMime?: string;
}

// ---------- create_literal_source ----------
export interface CreateLiteralSourceRequest {
  utf8: string;
}
export interface CreateLiteralSourceResponse {
  handle: SourceHandle;
  sizeBytes: number;
}

// ---------- preview_bytes ----------
export interface PreviewBytesRequest {
  handle: SourceHandle;
  offset: number;
  length: number;
}
export interface PreviewBytesResponse {
  base64Chunk: string;
  actualLength: number;
}

// ---------- release_source ----------
export interface ReleaseSourceRequest {
  handle: SourceHandle;
}

// ---------- set_graph ----------
export interface SetGraphRequest {
  graph: GraphDto;
}

// ---------- run_node ----------
export interface RunNodeRequest {
  nodeId: NodeId;
  previewOnly: boolean;
}
export interface RunNodeResponse {
  outputHandle: SourceHandle;
  outputSizeBytes: number;
  durationMs: number;
}

// ---------- cancel_node ----------
export interface CancelNodeRequest {
  nodeId: NodeId;
}

// ---------- history ----------
export interface SnapshotDto {
  id: SnapshotId;
  parent: SnapshotId | null;
  nodeId: NodeId;
  operationId: string;
  operationVersion: string;
  params: unknown;
  inputContentHash: string;
  outputContentHash: string | null;
}

export interface JumpToSnapshotRequest {
  snapshotId: SnapshotId;
}

// ---------- recipe export/import (⏳ не подключено) ----------
export interface ExportRecipeRequest {
  graph: GraphDto;
  targetPath: string;
}
export interface ImportRecipeRequest {
  sourcePath: string;
}
export interface ImportRecipeResponse {
  graph: GraphDto;
  missingOperations: string[];
}
export interface ImportCyberChefRecipeRequest {
  sourcePath: string;
}
export interface ImportCyberChefRecipeResponse {
  graph: GraphDto;
  unmappedOperations: { cyberChefId: string; reason: string }[];
}

// ---------- plugins ----------
export type PluginCapability = "filesystem_read" | "filesystem_write" | "network";

export interface PluginManifestDto {
  id: PluginId;
  name: string;
  version: string;
  author: string;
  signatureValid: boolean;
  requestedCapabilities: PluginCapability[];
  grantedCapabilities: PluginCapability[];
}

export interface InstallPluginRequest {
  wasmPath: string;
  manifestPath: string;
}
export interface GrantCapabilityRequest {
  pluginId: PluginId;
  capability: PluginCapability;
}
export interface RevokeCapabilityRequest {
  pluginId: PluginId;
  capability: PluginCapability;
}

// ---------- events ----------
export interface OpProgressEvent {
  nodeId: NodeId;
  bytesProcessed: number;
  bytesTotal: number | null;
}
export interface OpMemoryWarningEvent {
  nodeId: NodeId;
  estimatedMb: number;
}
export interface GraphInvalidatedEvent {
  staleNodeIds: NodeId[];
}
