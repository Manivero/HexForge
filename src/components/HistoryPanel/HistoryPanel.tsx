import * as React from "react";
import { useAppStore } from "@/store/useAppStore";

/**
 * HistoryPanel — журнал Time-Travel (FR-4.1): снапшоты в порядке записи,
 * newest-first; клик по строке — прыжок (jump_to_snapshot: lineage-реплей
 * из корневого источника + перенос головы истории). Ветвление из
 * произвольной точки появится вместе с визуализацией DAG истории.
 */
export function HistoryPanel() {
  const snapshots = useAppStore((s) => s.snapshots);
  const jumpingSnapshotId = useAppStore((s) => s.jumpingSnapshotId);
  const jumpToSnapshot = useAppStore((s) => s.jumpToSnapshot);

  // Newest-first: последние действия сверху, как в Git log.
  const rows = React.useMemo(() => [...snapshots].reverse(), [snapshots]);

  return (
    <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
      <header className="mb-2 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-text-secondary">History</h2>
        <span className="text-2xs text-text-muted">{snapshots.length} snapshot(s)</span>
      </header>

      {rows.length === 0 ? (
        <p className="text-xs text-text-muted">
          История пуста — запустите узел, чтобы создать первый снапшот.
        </p>
      ) : (
        <ol className="flex max-h-56 flex-col gap-1 overflow-y-auto">
          {rows.map((snap, index) => {
            const isJumping = jumpingSnapshotId === snap.id;
            return (
              <li key={snap.id}>
                <button
                  onClick={() => void jumpToSnapshot(snap.id)}
                  disabled={jumpingSnapshotId !== null}
                  title={`Прыжок к снапшоту ${snap.id}`}
                  className={[
                    "flex w-full items-center justify-between gap-2 rounded-md border px-2 py-1.5 text-left",
                    "transition-colors duration-fast ease-out-expo",
                    jumpingSnapshotId === snap.id
                      ? "border-border-focus bg-surface-2"
                      : "border-transparent hover:border-border-default hover:bg-surface-2",
                    "disabled:cursor-not-allowed disabled:opacity-60",
                  ].join(" ")}
                >
                  <span className="font-mono text-xs text-text-primary">
                    {isJumping ? "replaying…" : snap.operationId}
                  </span>
                  <span className="flex shrink-0 items-center gap-2 font-mono text-2xs text-text-muted">
                    <span>v{snap.operationVersion}</span>
                    <span>#{rows.length - index}</span>
                    <span>{snap.nodeId.slice(0, 8)}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
