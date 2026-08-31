// Чистая логика обхода Node Graph без zustand/tauri-зависимостей —
// выделяется из store, чтобы покрываться юнит-тестами на node:test
// (fe-tests/graphWalk.test.mjs) и переиспользоваться будущими панелями.

import type { NodeId, OperationNodeDto } from "./ipc-contract";

/**
 * Поднимается от `fromId` по входной цепочке до корня (узел без inputs).
 *
 * Инварианты:
 * - граф ацикличен по контракту (DAG проверяется на бэкенде), но функция
 *   защищена visited-множеством: повреждённое состояние с циклом завершится
 *   за конечное число шагов вместо зависания UI;
 * - ссылка на отсутствующий узел трактуется как «корень не найден».
 *
 * @returns id корневого узла или null, если стартовый узел отсутствует.
 */
export function findRootId(
  nodes: Record<string, OperationNodeDto>,
  fromId: NodeId | null,
): string | null {
  if (!fromId) return null;

  const visited = new Set<NodeId>();
  let cursor: OperationNodeDto | undefined = nodes[fromId];
  while (cursor && cursor.inputs.length > 0) {
    if (visited.has(cursor.id)) {
      return null; // цикл — вне контракта, но не зависаем
    }
    visited.add(cursor.id);
    // Для merge-узлов берём первый вход для детерминированного поиска корня (single-source legacy).
    // Multi-source: используйте findRootIds для всех корней.
    const nextId: string | undefined = cursor.inputs[0];
    cursor = nextId !== undefined ? nodes[nextId] : undefined;
  }
  return cursor ? cursor.id : null;
}

export function findRootIds(
  nodes: Record<string, OperationNodeDto>,
  fromId: NodeId | null,
): string[] {
  if (!fromId) return [];
  const visited = new Set<NodeId>();
  const roots = new Set<string>();
  const stack: string[] = [fromId];
  while (stack.length > 0) {
    const id = stack.pop()!;
    if (visited.has(id)) continue;
    visited.add(id);
    const node = nodes[id];
    if (!node) continue;
    if (node.inputs.length === 0) {
      roots.add(id);
    } else {
      for (const inp of node.inputs) stack.push(inp);
    }
  }
  return [...roots];
}

export interface LayoutNode {
  id: string;
  depth: number;
}

/**
 * Порядок отрисовки канваса: BFS от корней по детям, «сироты» (недостижимые
 * от корней) — в конце. Чистая функция: замена на полноценный layout не меняет
 * API потребителя.
 */
export function layoutOrder(nodes: Record<string, OperationNodeDto>): LayoutNode[] {
  const children = new Map<string, string[]>();
  const roots: string[] = [];
  for (const node of Object.values(nodes)) {
    if (node.inputs.length === 0) roots.push(node.id);
    for (const input of node.inputs) {
      const list = children.get(input);
      if (list) list.push(node.id);
      else children.set(input, [node.id]);
    }
  }

  const out: LayoutNode[] = [];
  const seen = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const id = queue.shift();
    if (id === undefined || seen.has(id)) continue;
    seen.add(id);
    out.push({ id, depth: depthOf(id, nodes) });
    queue.push(...(children.get(id) ?? []));
  }
  // Сироты — недостижимые узлы (повреждённое состояние), рисуем последними.
  for (const id of Object.keys(nodes)) {
    if (!seen.has(id)) out.push({ id, depth: 0 });
  }
  return out;
}

function depthOf(id: string, nodes: Record<string, OperationNodeDto>): number {
  // Для merge берём максимальную глубину среди всех входов (FR-1.4)
  const memo = new Map<string, number>();
  const dfs = (nid: string, visiting: Set<string>): number => {
    if (memo.has(nid)) return memo.get(nid)!;
    if (visiting.has(nid)) return 0; // цикл — 0
    const node = nodes[nid];
    if (!node || node.inputs.length === 0) {
      memo.set(nid, 0);
      return 0;
    }
    visiting.add(nid);
    let max = 0;
    for (const inp of node.inputs) {
      max = Math.max(max, dfs(inp, visiting));
    }
    visiting.delete(nid);
    const d = max + 1;
    memo.set(nid, d);
    return d;
  };
  return dfs(id, new Set());
}
