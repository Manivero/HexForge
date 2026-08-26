// Юнит-тесты чистого обхода графа (findRootId) — скомпилированный артефакт.
import { test } from "node:test";
import assert from "node:assert/strict";
import { findRootId, layoutOrder } from "../.fe-build/graphWalk.js";

function chain(ids) {
  const nodes = {};
  ids.forEach((id, i) => {
    nodes[id] = { id, inputs: i === 0 ? [] : [ids[i - 1]] };
  });
  return nodes;
}

test("линейная цепочка из трёх узлов: корень — первый", () => {
  const nodes = chain(["a", "b", "c"]);
  assert.equal(findRootId(nodes, "c"), "a");
  assert.equal(findRootId(nodes, "b"), "a");
  assert.equal(findRootId(nodes, "a"), "a");
});

test("одиночный узел без входов — сам корень", () => {
  const nodes = chain(["solo"]);
  assert.equal(findRootId(nodes, "solo"), "solo");
});

test("fork: обход по inputs[0] приводит к корню", () => {
  const nodes = {
    root: { id: "root", inputs: [] },
    a: { id: "a", inputs: ["root"] },
    b: { id: "b", inputs: ["root"] },
    ab: { id: "ab", inputs: ["a", "b"] }, // merge-узел
  };
  // Обход идёт только по inputs[0] (MVP-семантика линейной цепочки).
  assert.equal(findRootId(nodes, "ab"), "root");
  assert.equal(findRootId(nodes, "a"), "root");
});

test("ссылка на отсутствующий узел → null", () => {
  const nodes = { a: { id: "a", inputs: ["ghost"] } };
  assert.equal(findRootId(nodes, "a"), null);
  assert.equal(findRootId(nodes, "ghost"), null);
});

test("fromId отсутствует / null → null", () => {
  assert.equal(findRootId({}, "nope"), null);
  assert.equal(findRootId({ a: { id: "a", inputs: [] } }, null), null);
});

function mkN(id, inputs) {
  return { id, operationId: "op." + id, operationVersion: "1.0.0", params: {}, inputs };
}

test("layoutOrder: линейная цепочка — порядок корень→хвост", () => {
  const nodes = {
    a: mkN("a", []),
    b: mkN("b", ["a"]),
    c: mkN("c", ["b"]),
  };
  assert.deepEqual(
    layoutOrder(nodes).map((n) => n.id),
    ["a", "b", "c"]
  );
});

test("layoutOrder: fork — корень первым, дети после", () => {
  const nodes = {
    root: mkN("root", []),
    x: mkN("x", ["root"]),
    y: mkN("y", ["root"]),
  };
  const ids = layoutOrder(nodes).map((n) => n.id);
  assert.equal(ids[0], "root");
  // Порядок детей не специфицирован (Map/iter) — проверяем множество.
  assert.ok(ids.includes("x") && ids.includes("y"));
});

test("layoutOrder: сироты — последними с нулевой глубиной", () => {
  const nodes = {
    a: mkN("a", []),
    b: mkN("b", ["a"]),
    orphan: mkN("orphan", []), // корень сам по себе — НЕ сирота по определению
    lost: mkN("lost", ["ghost"]), // ссылка на отсутствующий узел — сирота
  };
  const items = layoutOrder(nodes);
  assert.equal(items[items.length - 1].id, "lost");
  assert.equal(items[items.length - 1].depth, 0);
  // a и b — до сироты
  const ai = items.findIndex((n) => n.id === "a");
  const li = items.findIndex((n) => n.id === "lost");
  assert.ok(ai < li);
});

test("layoutOrder: пустой граф — пустой список", () => {
  assert.deepEqual(layoutOrder({}), []);
});

