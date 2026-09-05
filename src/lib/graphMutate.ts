// Чистые мутации Node Graph (без zustand/tauri) — покрытие node:test.

import type { NodeId, OperationNodeDto, SourceHandle } from "./ipc-contract";

export interface RemoveResult {
  nodes: Record<string, OperationNodeDto>;
  removed: boolean;
}

/**
 * Удаляет узел из графа, «мостом» переподключая его прямых детей к первому
 * родителю удаляемого (dataflow сохраняется: a → b → c, delete b ⇒ a → c).
 *
 * Правила:
 * - дети без родителя у удаляемого (корень) становятся новыми корнями
 *   (inputs очищаются);
 * - дубли ссылок на мост устраняются (Set), самоссылка не создаётся;
 * - узлы, не связанные с удаляемым, остаются байт-в-байт теми же объектами.
 */
export function removeNode(nodes: Record<string, OperationNodeDto>, id: NodeId): RemoveResult {
  const target = nodes[id];
  if (!target) return { nodes, removed: false };

  const bridgeIds: string[] = [...target.inputs];

  const next: Record<string, OperationNodeDto> = {};
  for (const [nid, node] of Object.entries(nodes)) {
    if (nid === id) continue;

    if (!node.inputs.includes(id)) {
      // Не связан с удаляемым — копия как есть.
      next[nid] = node;
      continue;
    }

    // Ребёнок удаляемого: убираем ссылку и, если есть мост (N-ary), вставляем все
    // родителей удаляемого (дедуп, самоссылку не создаём) — FR-1.4.
    const rebuilt: string[] = [];
    for (const bid of bridgeIds) {
      if (bid !== nid && !rebuilt.includes(bid)) {
        rebuilt.push(bid);
      }
    }
    for (const inp of node.inputs) {
      if (inp !== id && !rebuilt.includes(inp)) {
        rebuilt.push(inp);
      }
    }
    next[nid] = { ...node, inputs: rebuilt };
  }

  return { nodes: next, removed: true };
}

/** Пустой граф — утилита для кнопки Clear. */
export function emptyGraph(): Record<string, never> {
  return {};
}

/**
 * Привязывает байтовый источник к корневому узлу, СОХРАНЯЯ его собственные
 * params операции (напр. alphabet у base64.decode) — перезаписывается только
 * ключ sourceHandle. Неизвестный корень → null (привязки нет).
 * Входной record не мутируется; незатронутые узлы shared by reference.
 */
export function bindSourceHandle(
  nodes: Record<string, OperationNodeDto>,
  rootId: NodeId,
  handle: SourceHandle,
): Record<string, OperationNodeDto> | null {
  const root = nodes[rootId];
  if (!root) return null;
  const prev =
    root.params !== null && typeof root.params === "object" && !Array.isArray(root.params)
      ? (root.params as Record<string, unknown>)
      : {};
  return {
    ...nodes,
    [rootId]: { ...root, params: { ...prev, sourceHandle: handle } },
  };
}
