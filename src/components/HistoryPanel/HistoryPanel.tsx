import * as React from "react";
import { useAppStore } from "@/store/useAppStore";
import type { SnapshotDto } from "@/lib/ipc-contract";

/**
 * HistoryPanel — журнал Time-Travel (FR-4.1) в виде ДЕРЕВА: снапшоты
 * раскладываются по parent-ссылкам (DFS от корней), ветки визуально
 * смещаются — «аналог Git», а не плоский undo-стек. Клик по узлу — прыжок
 * (jump_to_snapshot: lineage-реплей + перенос головы истории).
 */

interface TreeNode {
  snap: SnapshotDto;
  depth: number;
}

/** DFS-раскладка дерева истории: корни (parent=null) сверху, дети под родителем. */
function layoutTree(snapshots: SnapshotDto[]): TreeNode[] {
  const byId = new Map<string, SnapshotDto>(snapshots.map((s) => [s.id, s]));
  const children = new Map<string | null, SnapshotDto[]>();
  for (const snap of snapshots) {
    const key = snap.parent !== null && byId.has(snap.parent) ? snap.parent : null;
    const list = children.get(key);
    if (list) {
      list.push(snap);
    } else {
      children.set(key, [snap]);
    }
  }

  const out: TreeNode[] = [];
  const stack: Array<{ snap: SnapshotDto; depth: number }> = [];
  for (const root of [...(children.get(null) ?? [])].reverse()) {
    stack.push({ snap: root, depth: 0 });
  }
  while (stack.length > 0) {
    const { snap, depth } = stack.pop()!;
    out.push({ snap, depth });
    // Дети пушатся в обратном порядке, чтобы при LIFO-обходе шли по порядку.
    const kids = children.get(snap.id) ?? [];
    for (const kid of [...kids].reverse()) {
      stack.push({ snap: kid, depth: depth + 1 });
    }
  }
  return out;
}

export function HistoryPanel() {
  const snapshots = useAppStore((s) => s.snapshots);
  const jumpingSnapshotId = useAppStore((s) => s.jumpingSnapshotId);
  const jumpToSnapshot = useAppStore((s) => s.jumpToSnapshot);
  const selectedForDiff = useAppStore((s) => s.selectedForDiff);
  const diffText = useAppStore((s) => s.diffText);
  const diffLoading = useAppStore((s) => s.diffLoading);
  const diffError = useAppStore((s) => s.diffError);
  const selectForDiff = useAppStore((s) => s.selectForDiff);
  const clearDiffSelection = useAppStore((s) => s.clearDiffSelection);
  const diffSelected = useAppStore((s) => s.diffSelected);
  const restoreSnapshot = useAppStore((s) => s.restoreSnapshot);
  const runError = useAppStore((s) => s.runError);

  const rows = React.useMemo(() => layoutTree(snapshots), [snapshots]);

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
        <>
          <p className="mb-2 text-2xs text-text-muted">
            Клик — прыжок (реплей lineage). Запуск узла после прыжка создаёт ветку от выбранной
            точки (FR-4.1). Выберите два снапшота для diff (FR-4.3), Restore — прыжок с превью.
          </p>
          <ol className="flex max-h-56 flex-col gap-0.5 overflow-y-auto">
            {rows.map(({ snap, depth }) => {
              const isJumping = jumpingSnapshotId === snap.id;
              const isSelectedForDiff = selectedForDiff.includes(snap.id);
              const isBranchPoint =
                depth > 0 &&
                (() => {
                  const siblings = snapshots.filter((other) => other.parent === snap.parent);
                  return siblings.length > 1;
                })();
              const paramsStr = (() => {
                try {
                  const s = JSON.stringify(snap.params);
                  return s.length > 24 ? s.slice(0, 24) + "…" : s;
                } catch {
                  return String(snap.params);
                }
              })();

              return (
                <li key={snap.id} style={{ paddingLeft: `${depth * 14}px` }}>
                  <div
                    className={[
                      "flex w-full items-center justify-between gap-1 rounded-md border px-2 py-1 text-left",
                      "transition-colors duration-fast ease-out-expo",
                      isJumping
                        ? "border-border-focus bg-surface-2"
                        : isSelectedForDiff
                          ? "border-accent bg-surface-2"
                          : "border-transparent hover:border-border-default hover:bg-surface-2",
                    ].join(" ")}
                  >
                    <span className="flex min-w-0 items-center gap-1">
                      <input
                        type="checkbox"
                        checked={isSelectedForDiff}
                        onChange={() => selectForDiff(snap.id)}
                        title="Выбрать для diff"
                        className="h-3 w-3 rounded border-border-default"
                      />
                      <span
                        aria-hidden
                        className={[
                          "shrink-0 font-mono text-2xs",
                          isBranchPoint ? "text-accent" : "text-text-muted",
                        ].join(" ")}
                      >
                        {depth > 0 ? (isBranchPoint ? "├" : "│") : "●"}
                      </span>
                      <button
                        onClick={() => void jumpToSnapshot(snap.id)}
                        disabled={jumpingSnapshotId !== null}
                        title={`Прыжок к снапшоту ${snap.id}`}
                        className="truncate font-mono text-xs text-text-primary hover:text-accent disabled:opacity-60"
                      >
                        {isJumping ? "replaying…" : snap.operationId}
                      </button>
                    </span>
                    <span className="flex shrink-0 items-center gap-1 font-mono text-2xs text-text-muted">
                      <span title={snap.inputContentHash}>in:{snap.inputContentHash.slice(0, 6)}</span>
                      <span title={snap.outputContentHash ?? ""}>out:{(snap.outputContentHash ?? "").slice(0, 6)}</span>
                      <span title={paramsStr}>{paramsStr.slice(0, 8)}</span>
                      <span>v{snap.operationVersion}</span>
                      <button
                        onClick={() => void restoreSnapshot(snap.id)}
                        disabled={jumpingSnapshotId !== null}
                        title="Restore — прыжок с превью (single/multi-source)"
                        className="rounded border border-border-subtle px-1 py-0.5 text-2xs hover:border-accent hover:text-accent"
                      >
                        Restore
                      </button>
                    </span>
                  </div>
                </li>
              );
            })}
          </ol>
          <div className="mt-3 flex items-center gap-2">
            <button
              onClick={() => void diffSelected()}
              disabled={selectedForDiff.length !== 2 || diffLoading}
              className="rounded border border-border-default px-2 py-1 text-xs hover:border-accent disabled:opacity-50"
            >
              {diffLoading ? "Diff…" : `Diff (${selectedForDiff.length}/2)`}
            </button>
            <button
              onClick={() => clearDiffSelection()}
              disabled={selectedForDiff.length === 0 && !diffText && !diffError}
              className="rounded border border-border-subtle px-2 py-1 text-2xs text-text-muted hover:text-text-primary disabled:opacity-50"
            >
              Clear
            </button>
            {diffError && <span className="font-mono text-2xs text-status-error">{diffError}</span>}
          </div>
          {runError && <p className="mt-2 font-mono text-2xs text-status-error">{runError}</p>}
          {diffText && (
            <pre className="mt-2 max-h-32 overflow-auto rounded bg-surface-2 p-2 font-mono text-2xs text-text-primary" data-selectable>
              {diffText}
            </pre>
          )}
          {diffText === "equal\n" && <p className="mt-1 text-2xs text-text-muted">Snapshots are equal</p>}
        </>
      )}
    </section>
  );
}
