// Владение состоянием следует 03-INFORMATION-ARCHITECTURE.md §3: только
// graphSlice попадает в экспортируемый .hexforge-recipe, всё остальное —
// эфемерное UI-состояние. Срезы ui/palette/operations/graph реализованы в
// Этапе 2; dataSlice добавляет первый сквозной поток данных по схеме из
// 05-IPC-CONTRACT.md §3: литерал → create_literal_source → debounced
// set_graph → run_node → preview_bytes → PreviewDock.

import { create } from "zustand";
import {
  listOperations,
  setGraph as ipcSetGraph,
  createLiteralSource,
  runNode as ipcRunNode,
  previewBytes as ipcPreviewBytes,
  listSnapshots as ipcListSnapshots,
  jumpToSnapshot as ipcJumpToSnapshot,
  cancelNode as ipcCancelNode,
  decodeBase64Chunk,
} from "@/lib/ipc";
import { HexForgeCommandError } from "@/lib/ipc-contract";
import type {
  NodeId,
  OperationDescriptor,
  OperationNodeDto,
  RunNodeResponse,
  SnapshotDto,
  SnapshotId,
  SourceHandle,
} from "@/lib/ipc-contract";
import { toHexDump, toLossyUtf8 } from "@/lib/bytes";

export type Theme = "dark" | "light";

interface UiSlice {
  theme: Theme;
  toggleTheme: () => void;
}

interface PaletteSlice {
  isPaletteOpen: boolean;
  openPalette: () => void;
  closePalette: () => void;
  togglePalette: () => void;
}

interface OperationsSlice {
  operations: OperationDescriptor[];
  operationsLoading: boolean;
  operationsError: string | null;
  loadOperations: () => Promise<void>;
}

interface GraphSlice {
  nodes: Record<string, OperationNodeDto>;
  selectedNodeId: string | null;
  selectNode: (nodeId: string | null) => void;
  /** Добавляет узел операции, подключённый к текущему выделенному узлу
   * (или создаёт корень, если граф пуст) — прямая реализация FR-2.3. */
  addOperationNode: (operation: OperationDescriptor) => string;
  /** Мерджит patch в params выбранного/указанного узла (FR-3.2:
   * форма параметров InspectorPanel) и планирует debounced set_graph. */
  updateNodeParams: (nodeId: string, patch: Record<string, unknown>) => void;
  /** Привязывает созданный источник к корню цепочки выделенного узла
   * (params.sourceHandle корневого узла, конвенция 05-IPC-CONTRACT.md). */
  assignSourceToRoot: () => boolean;
  syncGraphToBackend: () => Promise<void>;
}

/** Первый сквозной data-срез: источник байтов + выполнение + превью. */
interface DataSlice {
  sourceHandle: SourceHandle | null;
  sourceSizeBytes: number | null;
  creatingSource: boolean;
  createSource: (text: string) => Promise<boolean>;

  /** Монотонный счётчик мутаций графа (узлы/параметры) — базис
   * stale-инвалидации результата превью (видимая часть FR-1.6). */
  graphVersion: number;
  runningNodeId: NodeId | null;
  lastRun: RunNodeResponse | null;
  /** graphVersion на момент последнего успешного запуска. */
  ranAtGraphVersion: number | null;
  previewText: string | null;
  previewHex: string | null;
  /** true, если показаны не все байты результата (лимит окна превью). */
  previewTruncated: boolean;
  /** Постраничный HexViewer: смещение и байты текущей страницы. */
  hexOffset: number | null;
  hexBytes: Uint8Array | null;
  hexLoading: boolean;
  loadHexPage: (offset: number) => Promise<void>;
  /** Журнал истории (list_snapshots) в порядке записи; UI показывает
   * newest-first и инициирует прыжки (FR-4.1). */
  snapshots: SnapshotDto[];
  /** id снапшота, к которому сейчас идёт lineage-реплей. */
  jumpingSnapshotId: SnapshotId | null;
  runError: string | null;
  runSelectedNode: () => Promise<void>;
  /** Кооперативная отмена текущего запуска (cancel_node): планировщик
   * завершит цепочку ошибкой Cancelled на ближайшем чекпоинте. */
  cancelRunningNode: () => Promise<void>;
  jumpToSnapshot: (snapshotId: SnapshotId) => Promise<void>;
}

export type AppStore = UiSlice &
  PaletteSlice &
  OperationsSlice &
  GraphSlice &
  DataSlice;

function newNodeId(): string {
  return crypto.randomUUID();
}

function formatIpcError(err: unknown): string {
  if (err instanceof HexForgeCommandError) {
    return `${err.details.kind}: ${err.details.message}`;
  }
  return err instanceof Error ? err.message : String(err);
}

/** Обновляет журнал истории; ошибка не всплывает — список вторичен
 * относительно результата запуска/прыжка. */
async function loadSnapshots(set: (partial: Partial<AppStore>) => void): Promise<void> {
  try {
    const snapshots = await ipcListSnapshots();
    set({ snapshots });
  } catch {
    /* без бэкенда журнал недоступен — UI деградирует мягко */
  }
}

const PREVIEW_LENGTH_BYTES = 4096;

// Дебаунс set_graph — контракт требует не блокировать ввод: локальный стейт
// обновляется мгновенно, бэкенд получает граф через 120ms после последнего
// изменения (05-IPC-CONTRACT.md §3).
let syncTimer: ReturnType<typeof setTimeout> | undefined;
function scheduleBackendSync(store: AppStore): void {
  clearTimeout(syncTimer);
  syncTimer = setTimeout(() => {
    void store.syncGraphToBackend().catch(() => {
      /* без бэкенда (vite dev вне Tauri) молча пропускаем — UI деградирует
         мягко, ошибка проявится при первом реальном запуске узла */
    });
  }, 120);
}

export const useAppStore = create<AppStore>((set, get) => ({
  // ---- ui ----
  theme: "dark",
  toggleTheme: () =>
    set((s) => ({ theme: s.theme === "dark" ? "light" : "dark" })),

  // ---- palette ----
  isPaletteOpen: false,
  openPalette: () => set({ isPaletteOpen: true }),
  closePalette: () => set({ isPaletteOpen: false }),
  togglePalette: () => set((s) => ({ isPaletteOpen: !s.isPaletteOpen })),

  // ---- operations registry ----
  operations: [],
  operationsLoading: false,
  operationsError: null,
  loadOperations: async () => {
    set({ operationsLoading: true, operationsError: null });
    try {
      const operations = await listOperations();
      set({ operations, operationsLoading: false });
    } catch (err) {
      set({
        operationsLoading: false,
        operationsError: formatIpcError(err),
      });
    }
  },

  // ---- graph (MVP: линейная цепочка, N-арные merge-узлы — post-MVP UI) ----
  nodes: {},
  selectedNodeId: null,
  selectNode: (nodeId) => set({ selectedNodeId: nodeId }),
  addOperationNode: (operation) => {
    const id = newNodeId();
    const { selectedNodeId, nodes } = get();
    const node: OperationNodeDto = {
      id,
      operationId: operation.id,
      operationVersion: operation.version,
      params: {},
      inputs: selectedNodeId && nodes[selectedNodeId] ? [selectedNodeId] : [],
    };
    set((s) => ({
      nodes: { ...s.nodes, [id]: node },
      selectedNodeId: id,
      graphVersion: s.graphVersion + 1,
    }));
    scheduleBackendSync(get());
    return id;
  },
  updateNodeParams: (nodeId, patch) => {
    const node = get().nodes[nodeId];
    if (!node) {
      return;
    }
    // Контракт типизирует params как unknown; по факту это плоский объект
    // параметров JSON Schema. Не-объект (битый узел) заменяем на patch.
    const current =
      node.params !== null &&
      typeof node.params === "object" &&
      !Array.isArray(node.params)
        ? (node.params as Record<string, unknown>)
        : {};
    const nextParams = { ...current, ...patch };
    set((s) => ({
      graphVersion: s.graphVersion + 1,
      nodes: {
        ...s.nodes,
        [nodeId]: { ...node, params: nextParams },
      },
    }));
    scheduleBackendSync(get());
  },
  assignSourceToRoot: () => {
    const { selectedNodeId, nodes, sourceHandle } = get();
    if (!sourceHandle || !selectedNodeId) {
      return false;
    }
    // Поднимаемся по входной цепочке до корня (граф ацикличен — инвариант
    // ядра), привязываем источник ему.
    let cursor: OperationNodeDto | undefined = nodes[selectedNodeId];
    while (cursor && cursor.inputs.length > 0) {
      const nextId: string | undefined = cursor.inputs[0];
      cursor = nextId !== undefined ? nodes[nextId] : undefined;
    }
    if (!cursor || cursor.inputs.length !== 0) {
      return false;
    }
    const rootId = cursor.id;
    const root = nodes[rootId];
    if (!root) {
      return false;
    }
    set({
      nodes: { ...nodes, [rootId]: { ...root, params: { sourceHandle } } },
    });
    scheduleBackendSync(get());
    return true;
  },
  syncGraphToBackend: async () => {
    await ipcSetGraph({ nodes: get().nodes });
  },

  // ---- data: literal source ----
  sourceHandle: null,
  sourceSizeBytes: null,
  creatingSource: false,
  createSource: async (text) => {
    set({ creatingSource: true, runError: null });
    try {
      const resp = await createLiteralSource({ utf8: text });
      set({
        sourceHandle: resp.handle,
        sourceSizeBytes: resp.sizeBytes,
        creatingSource: false,
        // Новые байты инвалидируют результат предыдущего запуска.
        lastRun: null,
        ranAtGraphVersion: null,
        previewText: null,
        previewHex: null,
        previewTruncated: false,
        hexOffset: null,
        hexBytes: null,
      });
      get().assignSourceToRoot();
      return true;
    } catch (err) {
      set({ creatingSource: false, runError: formatIpcError(err) });
      return false;
    }
  },

  // ---- data: run + preview ----
  graphVersion: 0,
  runningNodeId: null,
  lastRun: null,
  ranAtGraphVersion: null,
  previewText: null,
  previewHex: null,
  previewTruncated: false,
  hexOffset: null,
  hexBytes: null,
  hexLoading: false,
  snapshots: [],
  jumpingSnapshotId: null,
  runError: null,
  runSelectedNode: async () => {
    const nodeId = get().selectedNodeId;
    if (!nodeId) {
      set({ runError: "Выберите узел для запуска (⌘K → операция)" });
      return;
    }
    set({ runningNodeId: nodeId, runError: null });
    try {
      await get().syncGraphToBackend();
      const res = await ipcRunNode({ nodeId, previewOnly: true });
      const pb = await ipcPreviewBytes({
        handle: res.outputHandle,
        offset: 0,
        length: PREVIEW_LENGTH_BYTES,
      });
      const bytes = decodeBase64Chunk(pb.base64Chunk);
      set({
        lastRun: res,
        ranAtGraphVersion: get().graphVersion,
        previewText: toLossyUtf8(bytes),
        previewHex: toHexDump(bytes),
        previewTruncated: pb.actualLength < res.outputSizeBytes,
        hexOffset: null,
        hexBytes: null,
        runningNodeId: null,
      });
      await loadSnapshots(set);
    } catch (err) {
      set({
        runningNodeId: null,
        runError: formatIpcError(err),
        lastRun: null,
        ranAtGraphVersion: null,
        previewText: null,
        previewHex: null,
        previewTruncated: false,
      });
    }
  },
  cancelRunningNode: async () => {
    const nodeId = get().runningNodeId;
    if (!nodeId) {
      return;
    }
    // Ответ самой команды (true/false — был ли найден запуск) не важен:
    // исход виден по завершению runSelectedNode (ошибка Cancelled).
    try {
      await ipcCancelNode(nodeId);
    } catch (err) {
      set({ runError: formatIpcError(err) });
    }
  },
  loadHexPage: async (offset) => {
    const lastRun = get().lastRun;
    if (!lastRun) {
      return;
    }
    const total = lastRun.outputSizeBytes;
    const page = Math.max(0, Math.min(offset, Math.max(0, total - 1)));
    set({ hexLoading: true, hexOffset: page });
    try {
      const pb = await ipcPreviewBytes({
        handle: lastRun.outputHandle,
        offset: page,
        length: PREVIEW_LENGTH_BYTES,
      });
      // Рейс-гард: пока грузили страницу, смещение могло измениться.
      if (get().hexOffset === page) {
        set({
          hexBytes: decodeBase64Chunk(pb.base64Chunk),
          hexLoading: false,
        });
      } else {
        set({ hexLoading: false });
      }
    } catch (err) {
      set({ hexLoading: false, runError: formatIpcError(err) });
    }
  },
  jumpToSnapshot: async (snapshotId) => {
    set({ jumpingSnapshotId: snapshotId, runError: null });
    try {
      // Бэкенд сам реплеит lineage от корневого источника и переносит
      // голову истории (FR-4.1); ответ — стандартный RunNodeResponse.
      const res = await ipcJumpToSnapshot({ snapshotId });
      const pb = await ipcPreviewBytes({
        handle: res.outputHandle,
        offset: 0,
        length: PREVIEW_LENGTH_BYTES,
      });
      const bytes = decodeBase64Chunk(pb.base64Chunk);
      set({
        lastRun: res,
        ranAtGraphVersion: get().graphVersion,
        previewText: toLossyUtf8(bytes),
        previewHex: toHexDump(bytes),
        previewTruncated: pb.actualLength < res.outputSizeBytes,
        hexOffset: null,
        hexBytes: null,
        jumpingSnapshotId: null,
      });
      await loadSnapshots(set);
    } catch (err) {
      set({ jumpingSnapshotId: null, runError: formatIpcError(err) });
    }
  },
}));
