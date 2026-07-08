// Типизированная граница фронтенд/бекенд. Компоненты никогда не вызывают
// `invoke()` напрямую — только функции из этого файла, чтобы ни один вызов
// не мог разойтись с ipc-contract.ts по имени команды или форме payload.

import { invoke } from "@tauri-apps/api/core";
import type {
  CreateLiteralSourceRequest,
  CreateLiteralSourceResponse,
  GraphDto,
  HexForgeError,
  OpenFileRequest,
  OpenFileResponse,
  OperationDescriptor,
  PluginManifestDto,
  PreviewBytesRequest,
  PreviewBytesResponse,
  ReleaseSourceRequest,
  RunNodeRequest,
  RunNodeResponse,
  SnapshotDto,
} from "./ipc-contract";
import { HexForgeCommandError } from "./ipc-contract";

/** Оборачивает invoke, транслируя Rust HexForgeError (reject-объект) в
 * типизированный HexForgeCommandError вместо голого unknown-исключения. */
async function call<T>(command: string, payload?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, payload);
  } catch (raw) {
    if (isHexForgeError(raw)) {
      throw new HexForgeCommandError(raw);
    }
    throw raw;
  }
}

function isHexForgeError(value: unknown): value is HexForgeError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    "message" in value
  );
}

export function greet(name: string): Promise<string> {
  return call<string>("greet", { name });
}

export function listOperations(): Promise<OperationDescriptor[]> {
  return call<OperationDescriptor[]>("list_operations");
}

export function openFile(req: OpenFileRequest): Promise<OpenFileResponse> {
  return call<OpenFileResponse>("open_file", { req });
}

export function createLiteralSource(
  req: CreateLiteralSourceRequest,
): Promise<CreateLiteralSourceResponse> {
  return call<CreateLiteralSourceResponse>("create_literal_source", { req });
}

export function previewBytes(req: PreviewBytesRequest): Promise<PreviewBytesResponse> {
  return call<PreviewBytesResponse>("preview_bytes", { req });
}

export function releaseSource(req: ReleaseSourceRequest): Promise<boolean> {
  return call<boolean>("release_source", { req });
}

export function setGraph(graph: GraphDto): Promise<void> {
  return call<void>("set_graph", { req: { graph } });
}

export function runNode(req: RunNodeRequest): Promise<RunNodeResponse> {
  return call<RunNodeResponse>("run_node", { req });
}

export function cancelNode(nodeId: string): Promise<boolean> {
  return call<boolean>("cancel_node", { req: { nodeId } });
}

export function listSnapshots(): Promise<SnapshotDto[]> {
  return call<SnapshotDto[]>("list_snapshots");
}

export function listPlugins(): Promise<PluginManifestDto[]> {
  return call<PluginManifestDto[]>("list_plugins");
}

/** Декодирует base64Chunk из preview_bytes в Uint8Array для HexViewer/TextPreview. */
export function decodeBase64Chunk(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
