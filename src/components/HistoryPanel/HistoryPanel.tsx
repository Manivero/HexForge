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

const ID_SLICE = 8;

export function HistoryPanel() {
  const snapshots = useAppStore((s) => s.snapshots);
  const jumpingSnapshotId = useAppStore((s) => s.jumpingSnapshotId);
  const jumpToSnapshot = useAppStore((s) => s.jumpToSnapshot);

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
            Клик — прыжок (реплей lineage). Запуск узла после прыжка создаёт
            ветку от выбранной точки (FR-4.1).
          </p>
          <ol className="flex max-h-56 flex-col gap-0.5 overflow-y-auto">
            {rows.map(({ snap, depth }) => {
              const isJumping = jumpingSnapshotId === snap.id;
              const isBranchPoint =
                depth > 0 &&
                (() => {
                  // Точка ветвления: у этого снапшота есть "сиблинг" слева —
                  // то есть родитель имеет более одного ребёнка.
                  const siblings = snapshots.filter(
                    (other) => other.parent === snap.parent,
                  );
                  return siblings.length > 1;
                })();

              return (
                <li
                  key={snap.id}
                  style={{ paddingLeft: `${depth * 14}px` }}
                >
                  <button
                    onClick={() => void jumpToSnapshot(snap.id)}
                    disabled={jumpingSnapshotId !== null}
                    title={`Прыжок к снапшоту ${snap.id}`}
                    className={[
                      "flex w-full items-center justify-between gap-2 rounded-md border px-2 py-1 text-left",
                      "transition-colors duration-fast ease-out-expo",
                      jumpingSnapshotId === snap.id
                        ? "border-border-focus bg-surface-2"
                        : "border-transparent hover:border-border-default hover:bg-surface-2",
                      "disabled:cursor-not-allowed disabled:opacity-60",
                    ].join(" ")}
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        aria-hidden
                        className={[
                          "shrink-0 font-mono text-2xs",
                          isBranchPoint ? "text-accent" : "text-text-muted",
                        ].join(" ")}
                      >
                        {depth > 0 ? (isBranchPoint ? "├" : "│") : "●"}
                      </span>
                      <span className="truncate font-mono text-xs text-text-primary">
                        {isJumping ? "replaying…" : snap.operationId}
                      </span>
                    </span>
                    <span className="flex shrink-0 items-center gap-2 font-mono text-2xs text-text-muted">
                      <span>v{snap.operationVersion}</span>
                      <span>{snap.nodeId.slice(0, ID_SLICE)}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ol>
        </>
      )}
    </section>
  );
}
