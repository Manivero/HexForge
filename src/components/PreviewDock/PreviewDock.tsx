import * as React from "react";
import { useAppStore } from "@/store/useAppStore";
import { buildHexRows, formatAddr } from "@/lib/bytes";

type PreviewMode = "text" | "hex";

const PAGE_BYTES = 4096;

/**
 * PreviewDock — просмотр результата последнего запуска (вторая половина
 * потока 05-IPC-CONTRACT.md §3). TEXT — первые 4 КиБ lossy-UTF8; HEX —
 * постраничный viewer по 4 КиБ с навигацией ◀▶ и переходом по смещению
 * (бэкенд гарантирует лимит 1 МБ на запрос и clamp смещения).
 */
export function PreviewDock() {
  const [mode, setMode] = React.useState<PreviewMode>("text");
  const [editAddr, setEditAddr] = React.useState("");
  const [editValue, setEditValue] = React.useState("");
  const lastRun = useAppStore((s) => s.lastRun);
  const runningNodeId = useAppStore((s) => s.runningNodeId);
  const previewText = useAppStore((s) => s.previewText);
  const previewTruncated = useAppStore((s) => s.previewTruncated);
  const isStale = useAppStore((s) => s.lastRun !== null && s.ranAtGraphVersion !== s.graphVersion);

  // Hex-пагинация: страница грузится лениво при входе в режим/смене offset.
  const hexOffset = useAppStore((s) => s.hexOffset);
  const hexBytes = useAppStore((s) => s.hexBytes);
  const hexLoading = useAppStore((s) => s.hexLoading);
  const loadHexPage = useAppStore((s) => s.loadHexPage);
  const patchViewedByte = useAppStore((s) => s.patchViewedByte);

  const applyPatch = () => {
    const addr = Number.parseInt(editAddr, 16);
    if (Number.isNaN(addr)) return;
    void patchViewedByte(addr, editValue.trim()).then((ok) => {
      if (ok) setEditValue("");
    });
  };

  React.useEffect(() => {
    if (mode === "hex" && lastRun !== null && hexBytes === null && !hexLoading) {
      void loadHexPage(hexOffset ?? 0);
    }
  }, [mode, lastRun, hexBytes, hexLoading, hexOffset, loadHexPage]);

  const total = lastRun?.outputSizeBytes ?? 0;
  const offsetNow = hexOffset ?? 0;
  const pageEnd = offsetNow + (hexBytes?.length ?? 0);

  const gotoOffset = (raw: string) => {
    const parsed = Number.parseInt(raw, 16);
    if (!Number.isNaN(parsed)) {
      void loadHexPage(parsed);
    }
  };

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

      {mode === "hex" && lastRun !== null && (
        <div className="mb-2 flex flex-wrap items-center gap-2 font-mono text-2xs text-text-muted">
          <button
            onClick={() => void loadHexPage(offsetNow - PAGE_BYTES)}
            disabled={hexLoading || offsetNow === 0}
            className="rounded-sm border border-border-default px-2 py-0.5 enabled:hover:text-text-primary disabled:opacity-40"
          >
            ◀ prev
          </button>
          <button
            onClick={() => void loadHexPage(offsetNow + (hexBytes?.length ?? PAGE_BYTES))}
            disabled={hexLoading || pageEnd >= total}
            className="rounded-sm border border-border-default px-2 py-0.5 enabled:hover:text-text-primary disabled:opacity-40"
          >
            next ▶
          </button>
          <input
            data-selectable
            key={offsetNow}
            defaultValue={offsetNow.toString(16)}
            placeholder="offset(hex)"
            onKeyDown={(e) => {
              if (e.key === "Enter") gotoOffset(e.currentTarget.value);
            }}
            onBlur={(e) => gotoOffset(e.currentTarget.value)}
            className={[
              "w-28 rounded-sm border border-border-default bg-surface-2 px-1.5 py-0.5",
              "outline-none focus:border-border-focus",
            ].join(" ")}
          />
          <span>
            {formatAddr(offsetNow)}–{formatAddr(pageEnd)} / {formatAddr(total)}
          </span>
          {lastRun !== null && (
            <>
              <span className="text-text-muted">|</span>
              <label className="flex items-center gap-1">
                byte@
                <input
                  data-selectable
                  value={editAddr}
                  onChange={(e) => setEditAddr(e.target.value)}
                  placeholder="addr"
                  className={[
                    "w-20 rounded-sm border border-border-default bg-surface-2 px-1.5 py-0.5",
                    "outline-none focus:border-border-focus",
                  ].join(" ")}
                />
              </label>
              <input
                data-selectable
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") applyPatch();
                }}
                placeholder="hex pairs"
                title="Hex-пары: deadbeef или de ad (≤4КБ)"
                className={[
                  "w-28 rounded-sm border border-border-default bg-surface-2 px-1.5 py-0.5",
                  "outline-none focus:border-border-focus",
                ].join(" ")}
              />
              <button
                onClick={applyPatch}
                disabled={
                  !/^[0-9a-fA-F]+$/.test(editValue.replace(/\s+/g, "")) ||
                  editValue.replace(/\s+/g, "").length % 2 !== 0 ||
                  editAddr === ""
                }
                className="rounded-sm border border-border-default px-2 py-0.5 enabled:hover:text-accent disabled:opacity-40"
              >
                patch
              </button>
            </>
          )}
          {hexLoading && <span className="text-accent">loading…</span>}
        </div>
      )}

      {mode === "hex" && lastRun !== null ? (
        hexBytes !== null && hexBytes.length > 0 ? (
          <div
            data-selectable
            className="max-h-56 min-h-16 overflow-auto rounded-md bg-surface-2 p-3 font-mono text-2xs leading-4 text-text-primary"
          >
            {buildHexRows(hexBytes, offsetNow).map((row) => (
              <div key={row.addr} className="whitespace-pre">
                <span className="text-text-muted">{formatAddr(row.addr).slice(2)}</span>
                {"  "}
                {row.hex}
                {"  "}
                <span className="text-text-secondary">{row.ascii}</span>
              </div>
            ))}
          </div>
        ) : (
          <pre
            data-selectable
            className="min-h-16 rounded-md bg-surface-2 p-3 font-mono text-xs text-text-muted"
          >
            {hexLoading ? "loading…" : "(пустой вывод)"}
          </pre>
        )
      ) : (
        <pre
          data-selectable
          className={[
            "max-h-48 min-h-16 overflow-auto rounded-md bg-surface-2 p-3 font-mono text-xs",
            "text-text-primary whitespace-pre-wrap break-all",
          ].join(" ")}
        >
          {runningNodeId
            ? "Running…"
            : previewText !== null && previewText.length > 0
              ? previewText
              : lastRun === null
                ? "Нет результата — запустите выбранный узел."
                : "(пустой вывод)"}
        </pre>
      )}

      {mode === "text" && previewTruncated && (
        <p className="mt-1 text-2xs text-text-muted">
          Показаны первые {PAGE_BYTES} байт — полный просмотр в режиме HEX.
        </p>
      )}
    </section>
  );
}
