// Юнит-тесты мутаций графа (removeNode с «мостом») — скомпилированный артефакт.
import { test } from "node:test";
import assert from "node:assert/strict";
import { bindSourceHandle, removeNode } from "../.fe-build/graphMutate.js";

function mk(id, inputs) {
  return { id, operationId: `op.${id}`, operationVersion: "1.0.0", params: {}, inputs };
}

test("удаление среднего звена мостит цепочку a→b→c ⇒ a→c", () => {
  const nodes = { a: mk("a", []), b: mk("b", ["a"]), c: mk("c", ["b"]) };
  const res = removeNode(nodes, "b");
  assert.equal(res.removed, true);
  assert.equal(res.nodes.b, undefined);
  assert.deepEqual(res.nodes.c.inputs, ["a"]);
  assert.deepEqual(res.nodes.a.inputs, []);
});

test("удаление корня делает детей новыми корнями", () => {
  const nodes = { r: mk("r", []), x: mk("x", ["r"]), y: mk("y", ["r"]) };
  const res = removeNode(nodes, "r");
  assert.deepEqual(res.nodes.x.inputs, []);
  assert.deepEqual(res.nodes.y.inputs, []);
});

test("fork-ребёнок получает мост первой ссылкой, без дублей", () => {
  // merge-узел m уже ссылается на root и на b; удаление b (его родитель —
  // не root) НЕ должно вносить мост; проверяем чистый кейс:
  const nodes = {
    r: mk("r", []),
    b: mk("b", ["r"]),
    m: mk("m", ["r", "b"]), // второй вход — удаляемый
  };
  const res = removeNode(nodes, "b");
  // m ссылался на r (мост) и b: после удаления остаётся только r, дубля нет.
  assert.deepEqual(res.nodes.m.inputs, ["r"]);
});

test("самоссылка не создаётся при вырожденном графе", () => {
  const nodes = { a: mk("a", []), b: mk("b", ["a", "a"]) }; // дубль входа
  const res = removeNode(nodes, "a");
  assert.deepEqual(res.nodes.b.inputs, [], "мост a === удаляемому → очищено");
});

test("неизвестный id: removed=false, объект тот же (без копий)", () => {
  const nodes = { a: mk("a", []) };
  const res = removeNode(nodes, "nope");
  assert.equal(res.removed, false);
  assert.equal(res.nodes, nodes);
});

test("удаление merge-узла мостит всех родителей (N-ary, FR-1.4)", () => {
  const nodes = {
    a: mk("a", []),
    b: mk("b", []),
    m: mk("m", ["a", "b"]),
    d: mk("d", ["m"]),
  };
  const res = removeNode(nodes, "m");
  // Ребёнок d имел [m], m имел [a,b] → после удаления d должен иметь [a,b] (оба родителя)
  assert.deepEqual(new Set(res.nodes.d.inputs), new Set(["a", "b"]));
  assert.equal(res.nodes.m, undefined);
});

test("bindSourceHandle добавляет handle, сохраняя собственные params корня", () => {
  const root = mk("r", []);
  root.params = { alphabet: "url_safe" };
  const nodes = { r: root, x: mk("x", ["r"]) };
  const bound = bindSourceHandle(nodes, "r", "handle-1");
  assert.deepEqual(bound.r.params, { alphabet: "url_safe", sourceHandle: "handle-1" });
  // Вход не мутирован, соседи — те же объекты.
  assert.deepEqual(nodes.r.params, { alphabet: "url_safe" });
  assert.equal(bound.x, nodes.x);
});

test("bindSourceHandle перезаписывает только sourceHandle при повторной привязке", () => {
  const root = mk("r", []);
  root.params = { alphabet: "url_safe", sourceHandle: "old" };
  const bound = bindSourceHandle({ r: root }, "r", "new");
  assert.deepEqual(bound.r.params, { alphabet: "url_safe", sourceHandle: "new" });
});

test("bindSourceHandle заменяет не-объектные params на { sourceHandle }", () => {
  const root = mk("r", []);
  root.params = "мусор";
  const bound = bindSourceHandle({ r: root }, "r", "h");
  assert.deepEqual(bound.r.params, { sourceHandle: "h" });
});

test("bindSourceHandle на неизвестном корне возвращает null", () => {
  const nodes = { a: mk("a", []) };
  assert.equal(bindSourceHandle(nodes, "nope", "h"), null);
});
