import * as React from "react";
import { CommandPalette } from "@/components/CommandPalette/CommandPalette";
import { GraphCanvas } from "@/components/GraphCanvas/GraphCanvas";
import { HistoryPanel } from "@/components/HistoryPanel/HistoryPanel";
import { InputPanel } from "@/components/InputPanel/InputPanel";
import { InspectorPanel } from "@/components/InspectorPanel/InspectorPanel";
import { PreviewDock } from "@/components/PreviewDock/PreviewDock";
import { useAppStore } from "@/store/useAppStore";
import { t } from "@/lib/i18n";

/**
 * Этап 2: shell + Command Palette (⌘K) + сквозной data-поток
 * (05-IPC-CONTRACT.md §3) + GraphCanvas + InspectorPanel (FR-3.2) +
 * HistoryPanel Time-Travel (FR-4.1, jump_to_snapshot). ActivityBar и
 * полноценный DAG-canvas истории — следующие срезы; App остаётся тонкой
 * композицией.
 */
export function App() {
  const theme = useAppStore((s) => s.theme);
  const locale = useAppStore((s) => s.locale);
  const setLocale = useAppStore((s) => s.setLocale);
  const nodes = useAppStore((s) => s.nodes);
  const selectedNodeId = useAppStore((s) => s.selectedNodeId);
  const openPalette = useAppStore((s) => s.openPalette);
  const runSelectedNode = useAppStore((s) => s.runSelectedNode);
  const runningNodeId = useAppStore((s) => s.runningNodeId);
  const cancelRunningNode = useAppStore((s) => s.cancelRunningNode);
  const runError = useAppStore((s) => s.runError);
  const sourceHandle = useAppStore((s) => s.sourceHandle);
  const snapshots = useAppStore((s) => s.snapshots);
  const operationsError = useAppStore((s) => s.operationsError);
  const exportRecipe = useAppStore((s) => s.exportRecipe);
  const importRecipe = useAppStore((s) => s.importRecipe);

  React.useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  // Серверная инвалидация (FR-1.6, docs/05 §3): Rust считает downstream
  // изменённых узлов и эмитит graph://invalidated после set_graph.
  // В dev-режиме вне Tauri listen недоступен — деградирует молча.
  const applyServerStale = useAppStore((s) => s.applyServerStale);
  React.useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{ staleNodeIds: string[] }>("graph://invalidated", (event) => {
          applyServerStale(event.payload.staleNodeIds);
        }),
      )
      .then((un) => {
        if (cancelled) {
          un();
        } else {
          unlisten = un;
        }
      })
      .catch(() => {
        /* без нативного бэкенда события недоступны */
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [applyServerStale]);

  const nodeCount = Object.keys(nodes).length;
  const selectedNode = selectedNodeId ? nodes[selectedNodeId] : undefined;

  const handleExport = React.useCallback(async () => {
    const path = window.prompt(t(locale, "app.exportPrompt"), "recipe.hexforge");
    if (!path) return;
    await exportRecipe(path);
  }, [locale, exportRecipe]);

  const handleImport = React.useCallback(async () => {
    const path = window.prompt(t(locale, "app.importPrompt"), "recipe.hexforge");
    if (!path) return;
    await importRecipe(path);
  }, [locale, importRecipe]);

  return (
    <div className="flex h-screen w-screen flex-col bg-surface-0 text-text-primary">
      <div
        data-tauri-drag-region
        className="flex h-9 shrink-0 items-center justify-between border-b border-border-subtle px-3 text-2xs text-text-muted"
      >
        <span>HexForge</span>
        <div className="flex items-center gap-2">
          <button
            onClick={() => void handleExport()}
            className="rounded border border-border-subtle px-2 py-0.5 text-2xs hover:border-border-focus hover:text-accent"
            aria-label="export recipe"
          >
            {t(locale, "app.export")}
          </button>
          <button
            onClick={() => void handleImport()}
            className="rounded border border-border-subtle px-2 py-0.5 text-2xs hover:border-border-focus hover:text-accent"
            aria-label="import recipe"
          >
            {t(locale, "app.import")}
          </button>
          <button
            onClick={() => setLocale(locale === "en" ? "ru" : "en")}
            className="rounded border border-border-subtle px-2 py-0.5 text-2xs"
            aria-label="toggle locale"
          >
            {locale.toUpperCase()}
          </button>
        </div>
      </div>

      <main className="flex flex-1 items-start justify-center overflow-y-auto p-6">
        <div className="flex w-full max-w-3xl flex-col gap-4">
          {/* Workspace: канвас цепочки + действия */}
          <section className="rounded-lg border border-border-subtle bg-graph-grid p-4">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <div className="text-sm text-text-secondary">
                {nodeCount === 0
                  ? t(locale, "app.emptyGraph")
                  : t(locale, "app.nodes", { count: nodeCount })}
                {selectedNode && (
                  <span className="ml-2 font-mono text-xs text-text-muted">
                    {t(locale, "app.selected", { id: selectedNode.operationId })}
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
                  {t(locale, "app.addOperation")}
                  <kbd className="ml-2 rounded-sm border border-border-subtle bg-surface-2 px-1.5 py-0.5 text-2xs">
                    ⌘K
                  </kbd>
                </button>
                {runningNodeId && (
                  <button
                    onClick={() => void cancelRunningNode()}
                    className={[
                      "rounded-md border border-status-error bg-surface-1 px-3 py-1.5 text-xs",
                      "text-status-error transition-colors duration-fast ease-out-expo",
                      "hover:border-status-error hover:text-text-primary",
                    ].join(" ")}
                  >
                    {t(locale, "app.cancel")}
                  </button>
                )}
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
                  {runningNodeId ? t(locale, "app.running") : t(locale, "app.runNode")}
                </button>
              </div>
            </div>
            <GraphCanvas />
          </section>

          <InspectorPanel />
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            <InputPanel />
            <HistoryPanel />
          </div>
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
        <span>{t(locale, "app.version")}</span>
        <span className="flex gap-4">
          <span>{sourceHandle ? t(locale, "app.sourceReady") : t(locale, "app.noSource")}</span>
          <span>{t(locale, "app.snapshots", { count: snapshots.length })}</span>
        </span>
      </div>

      <CommandPalette />
    </div>
  );
}
