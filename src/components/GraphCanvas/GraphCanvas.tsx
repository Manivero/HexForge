import * as React from "react";
import { useAppStore } from "@/store/useAppStore";
import type { OperationNodeDto } from "@/lib/ipc-contract";

/**
 * GraphCanvas — первый визуальный срез DAG (роадмап MVP):
 * вертикальный рельс, корни сверху, BFS по исходящим рёбрам. N-арная
 * merge-раскладка появится вместе с hexforge-stream; раскладка — чистая
 * функция от nodes, её легко заменить полноценным layout без смены API.
 */

/** Порядок отрисовки: корни → BFS по детям; сироты — в конце. */
function layoutOrder(nodes: Record<string, OperationNodeDto>): string[] {
  const children = new Map<string, string[]>();
  const roots: string[] = [];
  for (const node of Object.values(nodes)) {
    if (node.inputs.length === 0) {
      roots.push(node.id);
    }
    for (const input of node.inputs) {
      const list = children.get(input);
      if (list) {
        list.push(node.id);
      } else {
        children.set(input, [node.id]);
      }
    }
  }

  const order: string[] = [];
  const seen = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const id = queue.shift();
    if (id === undefined || seen.has(id)) continue;
    seen.add(id);
    order.push(id);
    queue.push(...(children.get(id) ?? []));
  }
  for (const id of Object.keys(nodes)) {
    if (!seen.has(id)) order.push(id);
  }
  return order;
}

const ID_SLICE = 8;

export function GraphCanvas() {
  const nodes = useAppStore((s) => s.nodes);
  const selectedNodeId = useAppStore((s) => s.selectedNodeId);
  const selectNode = useAppStore((s) => s.selectNode);
  const runningNodeId = useAppStore((s) => s.runningNodeId);
  const deleteNode = useAppStore((s) => s.deleteNode);
  const staleNodeIds = useAppStore((s) => s.staleNodeIds);

  const order = React.useMemo(() => layoutOrder(nodes), [nodes]);
  const staleSet = React.useMemo(
    () => new Set(staleNodeIds),
    [staleNodeIds],
  );

  return (
    // Рельс слева: непрерывная вертикальная линия через все узлы цепочки.
    <div className="relative pl-6">
      {order.length > 0 && (
        <span
          aria-hidden
          className="absolute bottom-3 left-3 top-3 w-px bg-border-default"
        />
      )}
      <ol className="flex flex-col gap-1.5">
        {order.map((id) => {
          const node = nodes[id];
          if (!node) return null;
          const isSelected = id === selectedNodeId;
          const isRunning = id === runningNodeId;
          const isStale = staleSet.has(id);
          const sourceHandle =
            typeof node.params === "object" &&
            node.params !== null &&
            "sourceHandle" in node.params &&
            typeof node.params.sourceHandle === "string"
              ? node.params.sourceHandle
              : null;

          return (
            <li key={id} className="group relative">
              {/* Маркер узла на рельсе: корень без источника — idle, с
                  привязанным sourceHandle и все остальные — running/accent */}
              <span
                aria-hidden
                className={[
                  "absolute -left-4 top-1/2 h-2 w-2 -translate-y-1/2 rounded-full",
                  isSelected
                    ? "bg-accent"
                    : sourceHandle || node.inputs.length > 0
                      ? "bg-status-running"
                      : "bg-status-idle",
                ].join(" ")}
              />
              <div className="relative flex-1">
                <button
                onClick={() => selectNode(id)}
                data-node-id={id}
                className={[
                  "w-full rounded-md border px-3 py-2 text-left transition-colors duration-fast ease-out-expo",
                  isSelected
                    ? "border-border-focus bg-surface-2"
                    : "border-border-subtle bg-surface-2 hover:border-border-default",
                ].join(" ")}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-xs text-text-primary">
                    {node.operationId}
                    {isRunning && (
                      <span className="ml-2 text-status-running">running…</span>
                    )}
                    {isStale && !isRunning && (
                      <span className="ml-2 text-status-stale">stale</span>
                    )}
                  </span>
                  <span className="text-2xs text-text-muted">
                    v{node.operationVersion}
                  </span>
                </div>
                <div className="mt-0.5 flex items-center justify-between gap-2 font-mono text-2xs text-text-muted">
                  <span>{id.slice(0, ID_SLICE)}</span>
                  {sourceHandle && (
                    <span className="text-status-stale">
                      src {sourceHandle.slice(0, ID_SLICE)}
                    </span>
                  )}
                </div>
              </button>
              <button
                aria-label="Delete node"
                title="Удалить узел (дети мостятся к родителю)"
                onClick={(e) => {
                  e.stopPropagation();
                  deleteNode(id);
                }}
                className="absolute right-1 top-1 rounded-sm bg-surface-2 px-1.5 py-0.5 text-2xs leading-none text-text-muted opacity-0 transition-opacity duration-fast hover:text-status-error focus:opacity-100 [li:hover_&]:opacity-100"
              >
                ✕
              </button>
            </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
