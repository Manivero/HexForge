// Юнит-тесты чистого обхода графа (findRootId) — скомпилированный артефакт.
import { test } from "node:test";
import assert from "node:assert/strict";
import { findRootId } from "../.fe-build/graphWalk.js";

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
