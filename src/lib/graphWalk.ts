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
    const nextId: string | undefined = cursor.inputs[0];
    cursor = nextId !== undefined ? nodes[nextId] : undefined;
  }
  return cursor ? cursor.id : null;
}
