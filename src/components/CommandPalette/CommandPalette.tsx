import * as React from "react";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { useAppStore } from "@/store/useAppStore";
import { greet } from "@/lib/ipc";
import { fuzzyMatch } from "@/lib/fuzzyMatch";
import { buildAppCommands, operationsToCommands, type PaletteCommand } from "./commands";

const GROUP_LABELS: Record<PaletteCommand["groupId"], string> = {
  app: "Application",
  operations: "Operations",
};

/**
 * Command Palette — первичный интерфейс приложения (см. PRD FR-2.x,
 * 03-INFORMATION-ARCHITECTURE.md §1: "визуальные панели вторичны").
 * Глобально вызывается по ⌘K/Ctrl+K из любого места приложения.
 */
export function CommandPalette() {
  const isOpen = useAppStore((s) => s.isPaletteOpen);
  const openPalette = useAppStore((s) => s.openPalette);
  const closePalette = useAppStore((s) => s.closePalette);
  const toggleTheme = useAppStore((s) => s.toggleTheme);
  const operations = useAppStore((s) => s.operations);
  const operationsLoading = useAppStore((s) => s.operationsLoading);
  const loadOperations = useAppStore((s) => s.loadOperations);
  const addOperationNode = useAppStore((s) => s.addOperationNode);
  const clearGraph = useAppStore((s) => s.clearGraph);
  const deleteNode = useAppStore((s) => s.deleteNode);
  const selectedForDelete = useAppStore((s) => s.selectedNodeId);

  const [query, setQuery] = React.useState("");
  const [bridgeStatus, setBridgeStatus] = React.useState<string | null>(null);

  // Глобальный keybinding — работает из любого экрана, палитра не зависит
  // от того, какая панель сфокусирована (FR-2.1).
  React.useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const isMod = e.metaKey || e.ctrlKey;
      if (isMod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        useAppStore.getState().togglePalette();
      }
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);

  // Загрузка реестра операций — не пересрабатывает повторно, пока палитра
  // остаётся открытой: зависит только от `isOpen`, а не от `operations.length`.
  // (Раньше это было объединено с эффектом сброса ниже в одном useEffect с
  // deps `[isOpen, operations.length, ...]` — из-за этого, как только
  // `loadOperations()` резолвился и `operations.length` менялся с 0 на N
  // ПОКА isOpen оставался true, эффект перезапускался целиком и вызывал
  // `setQuery("")` посреди печати пользователя, стирая всё, что он успел
  // набрать за то время, пока список операций ещё грузился.)
  React.useEffect(() => {
    if (isOpen && operations.length === 0 && !operationsLoading) {
      void loadOperations();
    }
  }, [isOpen, operations.length, operationsLoading, loadOperations]);

  // Сброс query/bridgeStatus обязан происходить ровно один раз — в момент
  // ПЕРЕХОДА закрыто→открыто, а не при каждом изменении isOpen-зависимого
  // состояния, пока диалог остаётся открытым. `wasOpenRef` фиксирует именно
  // передний фронт открытия.
  const wasOpenRef = React.useRef(false);
  React.useEffect(() => {
    if (isOpen && !wasOpenRef.current) {
      setQuery("");
      setBridgeStatus(null);
    }
    wasOpenRef.current = isOpen;
  }, [isOpen]);

  const allCommands = React.useMemo<PaletteCommand[]>(() => {
    const appCommands = buildAppCommands({
      toggleTheme,
      clearGraph,
      deleteSelectedNode: () => deleteNode(selectedForDelete ?? ""),
      runGreetTest: () => {
        setBridgeStatus("Checking...");
        greet("Architect")
          .then((response) => setBridgeStatus(response))
          .catch((err) => setBridgeStatus(`Bridge error: ${String(err)}`));
      },
    });
    const opCommands = operationsToCommands(operations, (operation) => {
      addOperationNode(operation);
      closePalette();
    });
    return [...appCommands, ...opCommands];
  }, [
    toggleTheme,
    operations,
    addOperationNode,
    closePalette,
    clearGraph,
    deleteNode,
    selectedForDelete,
  ]);

  const filtered = React.useMemo(() => {
    if (query.trim().length === 0) return allCommands;
    return allCommands
      .map((cmd) => ({
        cmd,
        ...fuzzyMatch(query, `${cmd.label} ${cmd.keywords?.join(" ") ?? ""}`),
      }))
      .filter((r) => r.matched)
      .sort((a, b) => b.score - a.score)
      .map((r) => r.cmd);
  }, [allCommands, query]);

  const grouped = React.useMemo(() => {
    const groups = new Map<PaletteCommand["groupId"], PaletteCommand[]>();
    for (const cmd of filtered) {
      const list = groups.get(cmd.groupId) ?? [];
      list.push(cmd);
      groups.set(cmd.groupId, list);
    }
    return groups;
  }, [filtered]);

  return (
    <CommandDialog open={isOpen} onOpenChange={(open) => (open ? openPalette() : closePalette())}>
      <CommandInput
        placeholder="Type a command or search operations..."
        value={query}
        onValueChange={setQuery}
      />
      <CommandList>
        {bridgeStatus && (
          <div className="border-b border-border-subtle px-3 py-2 font-mono text-2xs text-accent">
            {bridgeStatus}
          </div>
        )}
        <CommandEmpty>
          {operationsLoading ? "Loading operations..." : "No results found."}
        </CommandEmpty>
        {Array.from(grouped.entries()).map(([groupId, commands]) => (
          <CommandGroup key={groupId} heading={GROUP_LABELS[groupId]}>
            {commands.map((cmd) => (
              <CommandItem
                key={cmd.id}
                value={cmd.id}
                onSelect={() => {
                  void cmd.run();
                }}
              >
                <span>{cmd.label}</span>
                {cmd.hint && <span className="text-2xs text-text-muted">{cmd.hint}</span>}
              </CommandItem>
            ))}
          </CommandGroup>
        ))}
      </CommandList>
    </CommandDialog>
  );
}
