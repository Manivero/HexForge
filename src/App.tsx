import * as React from "react";
import { CommandPalette } from "@/components/CommandPalette/CommandPalette";
import { GraphCanvas } from "@/components/GraphCanvas/GraphCanvas";
import { InputPanel } from "@/components/InputPanel/InputPanel";
import { PreviewDock } from "@/components/PreviewDock/PreviewDock";
import { useAppStore } from "@/store/useAppStore";

/**
 * Этап 2: минимальный shell + Command Palette (⌘K) + первый сквозной
 * data-поток по схеме 05-IPC-CONTRACT.md §3 + GraphCanvas (вертикальный
 * срез DAG). Полная разметка из 03-INFORMATION-ARCHITECTURE.md
 * (ActivityBar, InspectorPanel) подключается в следующих срезах —
 * App остаётся тонкой композицией.
 */
export function App() {
  const theme = useAppStore((s) => s.theme);
  const nodes = useAppStore((s) => s.nodes);
  const selectedNodeId = useAppStore((s) => s.selectedNodeId);
  const openPalette = useAppStore((s) => s.openPalette);
  const runSelectedNode = useAppStore((s) => s.runSelectedNode);
  const runningNodeId = useAppStore((s) => s.runningNodeId);
  const runError = useAppStore((s) => s.runError);
  const sourceHandle = useAppStore((s) => s.sourceHandle);
  const snapshotCount = useAppStore((s) => s.snapshotCount);
  const operationsError = useAppStore((s) => s.operationsError);

  React.useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  const nodeCount = Object.keys(nodes).length;
  const selectedNode = selectedNodeId ? nodes[selectedNodeId] : undefined;

  return (
    <div className="flex h-screen w-screen flex-col bg-surface-0 text-text-primary">
      <div
        data-tauri-drag-region
        className="flex h-9 shrink-0 items-center justify-center border-b border-border-subtle text-2xs text-text-muted"
      >
        HexForge
      </div>

      <main className="flex flex-1 items-start justify-center overflow-y-auto p-6">
        <div className="flex w-full max-w-3xl flex-col gap-4">
          {/* Workspace: канвас цепочки + действия */}
          <section className="rounded-lg border border-border-subtle bg-graph-grid p-4">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <div className="text-sm text-text-secondary">
                {nodeCount === 0
                  ? "Пустой граф — начните с ⌘K"
                  : `${nodeCount} node(s)`}
                {selectedNode && (
                  <span className="ml-2 font-mono text-xs text-text-muted">
                    selected: {selectedNode.operationId}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={openPalette}
                  className={[
                    "rounded-md border border-border-default bg-surface-1 px-3 py-1.5",
                    "text-xs text-text-primary transition-colors duration-fast ease-out-expo",
                    "hover:border-border-focus hover:text-accent",
                  ].join(" ")}
                >
                  Add operation
                  <kbd className="ml-2 rounded-sm border border-border-subtle bg-surface-2 px-1.5 py-0.5 text-2xs">
                    ⌘K
                  </kbd>
                </button>
                <button
                  onClick={() => void runSelectedNode()}
                  disabled={!selectedNodeId || runningNodeId !== null}
                  className={[
                    "rounded-md border border-border-default bg-surface-1 px-3 py-1.5 text-xs",
                    "text-text-primary transition-colors duration-fast ease-out-expo",
                    "enabled:hover:border-border-focus enabled:hover:text-accent",
                    "disabled:cursor-not-allowed disabled:opacity-50",
                  ].join(" ")}
                >
                  {runningNodeId ? "Running…" : "Run node"}
                </button>
              </div>
            </div>
            <GraphCanvas />
          </section>

          <InputPanel />
          <PreviewDock />

          {(runError || operationsError) && (
            <section className="rounded-lg border border-status-error bg-surface-1 px-4 py-3">
              <p className="font-mono text-2xs text-status-error" data-selectable>
                {runError ?? operationsError}
              </p>
            </section>
          )}
        </div>
      </main>

      <div className="flex h-6 shrink-0 items-center justify-between border-t border-border-subtle px-3 text-2xs text-text-muted">
        <span>HexForge v0.1.0 — Node Graph MVP</span>
        <span className="flex gap-4">
          <span>{sourceHandle ? "source ready" : "no source"}</span>
          <span>{snapshotCount} snapshot(s)</span>
        </span>
      </div>

      <CommandPalette />
    </div>
  );
}
