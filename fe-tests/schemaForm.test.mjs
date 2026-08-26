// Юнит-тесты парсера JSON Schema (FR-3.2) — скомпилированный артефакт.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  extractFields,
  asParamsObject,
} from "../.fe-build/schemaForm.js";

test("полная схема base64: enum + default", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      alphabet: {
        type: "string",
        enum: ["standard", "url_safe"],
        default: "standard",
      },
    },
  });
  assert.equal(fields.length, 1);
  assert.equal(fields[0].name, "alphabet");
  assert.equal(fields[0].type, "string");
  assert.deepEqual(fields[0].enumValues, ["standard", "url_safe"]);
  assert.equal(fields[0].hasDefault, true);
  assert.equal(fields[0].defaultValue, "standard");
});

test("подмножество типов маппится корректно", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      s: { type: "string" },
      b: { type: "boolean" },
      i: { type: "integer" },
      n: { type: "number" },
    },
  });
  const byName = Object.fromEntries(fields.map((f) => [f.name, f.type]));
  assert.deepEqual(byName, {
    s: "string",
    b: "boolean",
    i: "integer",
    n: "number",
  });
});

test("неизвестный тип → other, enum без него не мешает", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      weird: { type: "array" },
    },
  });
  assert.equal(fields.length, 1);
  assert.equal(fields[0].type, "other");
  assert.deepEqual(fields[0].enumValues, []);
});

test("мусор в properties пропускается без падения", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      good: { type: "boolean" },
      nullDef: null,
      arrDef: [1, 2],
      strDef: "oops",
    },
  });
  assert.equal(fields.length, 1);
  assert.equal(fields[0].name, "good");
});

test("не-объектная схема → пустой список", () => {
  for (const bad of [null, undefined, 42, "str", [], true]) {
    assert.deepEqual(extractFields(bad), []);
  }
});

test("properties отсутствует/пусто → пустой список", () => {
  assert.deepEqual(extractFields({}), []);
  assert.deepEqual(extractFields({ type: "object" }), []);
  assert.deepEqual(extractFields({ type: "object", properties: {} }), []);
});

test("asParamsObject: объект проходит, прочее становится {}", () => {
  assert.deepEqual(asParamsObject({ a: 1 }), { a: 1 });
  assert.deepEqual(asParamsObject(null), {});
  assert.deepEqual(asParamsObject("str"), {});
  assert.deepEqual(asParamsObject([1]), {});
});
