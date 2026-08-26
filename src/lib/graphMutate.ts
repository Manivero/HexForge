// Чистые мутации Node Graph (без zustand/tauri) — покрытие node:test.

import type { NodeId, OperationNodeDto } from "./ipc-contract";

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
export function removeNode(
  nodes: Record<string, OperationNodeDto>,
  id: NodeId,
): RemoveResult {
  const target = nodes[id];
  if (!target) return { nodes, removed: false };

  const bridgeId: string | undefined = target.inputs[0];

  const next: Record<string, OperationNodeDto> = {};
  for (const [nid, node] of Object.entries(nodes)) {
    if (nid === id) continue;

    if (!node.inputs.includes(id)) {
      // Не связан с удаляемым — копия как есть.
      next[nid] = node;
      continue;
    }

    // Ребёнок удаляемого: убираем ссылку и, если есть мост, вставляем его
    // первой ссылкой (дедуп через Set; самоссылку не создаём).
    const rebuilt: string[] = [];
    if (bridgeId !== undefined && bridgeId !== nid) {
      rebuilt.push(bridgeId);
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
