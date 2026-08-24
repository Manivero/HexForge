import * as React from "react";
import { useAppStore } from "@/store/useAppStore";

type PreviewMode = "text" | "hex";

/**
 * PreviewDock — просмотр результата последнего запуска (вторая половина
 * потока 05-IPC-CONTRACT.md §3: run_node → preview_bytes → PreviewDock).
 * Байты приходят base64-окном ≤4KB; previewTruncated сигнализирует обрезку.
 */
export function PreviewDock() {
  const [mode, setMode] = React.useState<PreviewMode>("text");
  const lastRun = useAppStore((s) => s.lastRun);
  const runningNodeId = useAppStore((s) => s.runningNodeId);
  const previewText = useAppStore((s) => s.previewText);
  const previewHex = useAppStore((s) => s.previewHex);
  const previewTruncated = useAppStore((s) => s.previewTruncated);
  // FR-1.6 (видимая часть): граф мутировал после запуска → результат stale.
  // Полная точечная инвалидация по downstream придёт с hexforge-stream.
  const isStale = useAppStore(
    (s) => s.lastRun !== null && s.ranAtGraphVersion !== s.graphVersion,
  );

  const body =
    mode === "text" ? (previewText ?? "") : (previewHex ?? "");

  return (
    <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
      <header className="mb-2 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-text-secondary">Preview</h2>
        <div className="flex items-center gap-2">
          {isStale && (
            <span className="rounded-sm border border-status-stale px-1.5 py-0.5 text-2xs text-status-stale">
              stale
            </span>
          )}
          {lastRun && (
            <span className="text-2xs text-text-muted">
              {lastRun.outputSizeBytes} B · {lastRun.durationMs} ms · handle{" "}
              {lastRun.outputHandle.slice(0, 8)}
            </span>
          )}
          <div className="flex overflow-hidden rounded-md border border-border-default">
            {(["text", "hex"] as const).map((m) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                className={[
                  "px-2 py-0.5 text-2xs transition-colors duration-fast",
                  mode === m
                    ? "bg-surface-2 text-text-primary"
                    : "text-text-muted hover:text-text-secondary",
                ].join(" ")}
              >
                {m.toUpperCase()}
                {previewTruncated && "+"}
              </button>
            ))}
          </div>
        </div>
      </header>
      <pre
        data-selectable
        className={[
          "min-h-16 max-h-48 overflow-auto rounded-md bg-surface-2 p-3 font-mono text-xs",
          "text-text-primary whitespace-pre-wrap break-all",
        ].join(" ")}
      >
        {runningNodeId
          ? "Running…"
          : body.length > 0
            ? body
            : lastRun === null
              ? "Нет результата — запустите выбранный узел."
              : "(пустой вывод)"}
      </pre>
      {previewTruncated && (
        <p className="mt-1 text-2xs text-text-muted">
          Показаны первые 4096 байт результата.
        </p>
      )}
    </section>
  );
}
