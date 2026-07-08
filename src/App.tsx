import * as React from "react";
import { CommandPalette } from "@/components/CommandPalette/CommandPalette";
import { useAppStore } from "@/store/useAppStore";

/**
 * Этап 2, срез 1: минимальный shell + Command Palette как первый и
 * центральный UI-компонент (см. EXECUTION PROTOCOL). Полная разметка из
 * 03-INFORMATION-ARCHITECTURE.md (ActivityBar, GraphCanvas, InspectorPanel,
 * PreviewDock, StatusBar) подключается в следующих срезах без изменения
 * этого файла на уровне архитектуры — App остаётся тонкой композицией.
 */
export function App() {
  const theme = useAppStore((s) => s.theme);
  const nodes = useAppStore((s) => s.nodes);
  const selectedNodeId = useAppStore((s) => s.selectedNodeId);
  const openPalette = useAppStore((s) => s.openPalette);

  React.useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  const nodeCount = Object.keys(nodes).length;

  return (
    <div className="flex h-screen w-screen flex-col bg-surface-0 text-text-primary">
      <div
        data-tauri-drag-region
        className="flex h-9 shrink-0 items-center justify-center border-b border-border-subtle text-2xs text-text-muted"
      >
        HexForge
      </div>

      <main className="flex flex-1 items-center justify-center">
        <div className="flex flex-col items-center gap-4 text-center">
          <div className="bg-graph-grid rounded-lg border border-border-subtle px-16 py-12">
            <p className="text-lg text-text-secondary">Node Graph Workspace</p>
            <p className="mt-1 text-sm text-text-muted">
              {nodeCount === 0
                ? "Пусто — начните с ⌘K"
                : `${nodeCount} node(s), selected: ${selectedNodeId?.slice(0, 8) ?? "none"}`}
            </p>
            <button
              onClick={openPalette}
              className={[
                "mt-6 rounded-md border border-border-default bg-surface-1 px-4 py-2",
                "text-sm text-text-primary transition-colors duration-fast ease-out-expo",
                "hover:border-border-focus hover:text-accent",
              ].join(" ")}
            >
              Open Command Palette
              <kbd className="ml-2 rounded-sm border border-border-subtle bg-surface-2 px-1.5 py-0.5 text-2xs">
                ⌘K
              </kbd>
            </button>
          </div>
        </div>
      </main>

      <div className="flex h-6 shrink-0 items-center border-t border-border-subtle px-3 text-2xs text-text-muted">
        HexForge v0.1.0 — Node Graph MVP
      </div>

      <CommandPalette />
    </div>
  );
}
