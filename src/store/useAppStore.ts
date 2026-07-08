// Владение состоянием следует 03-INFORMATION-ARCHITECTURE.md §3: только
// graphSlice попадает в экспортируемый .hexforge-recipe, всё остальное —
// эфемерное UI-состояние. historySlice/dataSlice/pluginSlice полностью
// расписаны в IA-документе и добавляются в этот store по мере реализации
// соответствующих Rust-команд (см. комментарий статуса в ipc-contract.ts);
// в этом срезе (Этап 2, Command Palette) реализованы ui/palette/operations
// и минимальный graph slice, достаточный для линейных (non-merge) рецептов.

import { create } from "zustand";
import type { OperationDescriptor, OperationNodeDto } from "@/lib/ipc-contract";
import { listOperations, setGraph as ipcSetGraph } from "@/lib/ipc";

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
  syncGraphToBackend: () => Promise<void>;
}

export type AppStore = UiSlice & PaletteSlice & OperationsSlice & GraphSlice;

function newNodeId(): string {
  return crypto.randomUUID();
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
        operationsError: err instanceof Error ? err.message : String(err),
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
    }));
    return id;
  },
  syncGraphToBackend: async () => {
    await ipcSetGraph({ nodes: get().nodes });
  },
}));
