import type { OperationDescriptor } from "@/lib/ipc-contract";

export type CommandGroupId = "app" | "operations";

export interface PaletteCommand {
  id: string;
  groupId: CommandGroupId;
  label: string;
  hint?: string;
  keywords?: string[];
  run: () => void | Promise<void>;
}

/** Статические команды приложения (не операции) — навигация, тема, greet-тест
 * моста IPC. Палитра дублирует ЛЮБУЮ навигацию приложения, панели вторичны
 * (см. FR-2.5, 03-INFORMATION-ARCHITECTURE.md §1). Список растёт по мере
 * добавления панелей (History, Plugins, Files) в последующих срезах. */
export interface AppActions {
  toggleTheme: () => void;
  runGreetTest: () => void;
  clearGraph: () => void;
  deleteSelectedNode: () => boolean;
}

export function buildAppCommands(actions: AppActions): PaletteCommand[] {
  return [
    {
      id: "app.toggle-theme",
      groupId: "app",
      label: "Toggle Theme",
      hint: "Dark / Light",
      keywords: ["theme", "dark", "light", "appearance"],
      run: actions.toggleTheme,
    },
    {
      id: "app.verify-bridge",
      groupId: "app",
      label: "Verify Rust Bridge (greet)",
      hint: "Sanity-check IPC",
      keywords: ["greet", "bridge", "ipc", "health", "ping"],
      run: actions.runGreetTest,
    },
    {
      id: "app.clear-graph",
      groupId: "app",
      label: "Clear Graph",
      hint: "Remove all nodes",
      keywords: ["clear", "graph", "reset", "nodes"],
      run: actions.clearGraph,
    },
    {
      id: "app.delete-selected",
      groupId: "app",
      label: "Delete Selected Node",
      hint: "Bridge children to parent",
      keywords: ["delete", "node", "remove", "selected"],
      run: actions.deleteSelectedNode,
    },
  ];
}

/** Маппинг операций реестра в команды палитры — FR-2.3: добавление операции
 * в граф прямо из палитры создаёт узел, подключённый к текущему выделенному. */
export function operationsToCommands(
  operations: OperationDescriptor[],
  onSelect: (operation: OperationDescriptor) => void,
): PaletteCommand[] {
  return operations.map((op) => ({
    id: `op.${op.id}`,
    groupId: "operations",
    label: op.displayName,
    hint: op.category,
    keywords: [op.category, op.id],
    run: () => onSelect(op),
  }));
}
