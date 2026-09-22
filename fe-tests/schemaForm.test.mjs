// Юнит-тесты парсера JSON Schema (FR-3.2) — скомпилированный артефакт.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  extractFields,
  asParamsObject,
  validateParams,
  formStateToParams,
  schemaToFormState,
} from "../.fe-build/lib/schemaForm.js";

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

// ===== New FR-3.2 tests =====

test("description is captured", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      key: { type: "string", description: "A key for encryption" },
    },
  });
  assert.equal(fields[0].description, "A key for encryption");
});

test("required fields are marked", () => {
  const fields = extractFields({
    type: "object",
    required: ["key", "mode"],
    properties: {
      key: { type: "string" },
      mode: { type: "string" },
      iv: { type: "string" },
    },
  });
  const requiredMap = Object.fromEntries(
    fields.map((f) => [f.name, f.required]),
  );
  assert.deepEqual(requiredMap, { key: true, mode: true, iv: false });
});

test("minimum / maximum captured for integer", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      m_cost: { type: "integer", minimum: 8, maximum: 65536, default: 19456 },
    },
  });
  assert.equal(fields[0].minimum, 8);
  assert.equal(fields[0].maximum, 65536);
  assert.equal(fields[0].hasDefault, true);
  assert.equal(fields[0].defaultValue, 19456);
});

test("oneOf maps to enum-like values", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      mode: {
        oneOf: [{ const: "ecb" }, { const: "cbc" }, { const: "ctr" }],
      },
    },
  });
  assert.deepEqual(fields[0].enumValues, ["ecb", "cbc", "ctr"]);
});

test("anyOf maps to enum-like values", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      variant: {
        anyOf: [{ const: "argon2id" }, { const: "argon2i" }, { const: "argon2d" }],
      },
    },
  });
  assert.deepEqual(fields[0].enumValues, ["argon2id", "argon2i", "argon2d"]);
});

test("nested object properties are captured recursively", () => {
  const fields = extractFields({
    type: "object",
    required: ["config"],
    properties: {
      config: {
        type: "object",
        properties: {
          key: { type: "string" },
          rounds: { type: "integer", minimum: 1, maximum: 100 },
        },
      },
    },
  });
  assert.equal(fields.length, 1);
  assert.equal(fields[0].type, "object");
  assert.equal(fields[0].children.length, 2);
  assert.equal(fields[0].children[0].name, "key");
  assert.equal(fields[0].children[1].name, "rounds");
  assert.equal(fields[0].children[1].minimum, 1);
  assert.equal(fields[0].children[1].maximum, 100);
});

// ===== Real HexForge operation schemas =====

test("real XOR schema: required key with description", () => {
  const fields = extractFields({
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string", description: "UTF-8 key, cycled over input" },
    },
  });
  assert.equal(fields.length, 1);
  assert.equal(fields[0].required, true);
  assert.equal(fields[0].description, "UTF-8 key, cycled over input");
});

test("real AES schema: required key + enum mode + description", () => {
  const fields = extractFields({
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string", description: "Hex-encoded key (32/48/64 hex chars)" },
      mode: { type: "string", enum: ["ecb", "cbc", "ctr"], default: "cbc" },
      iv: { type: "string", description: "Hex-encoded 16-byte IV" },
    },
  });
  assert.equal(fields.length, 3);
  const byName = Object.fromEntries(
    fields.map((f) => [f.name, f]),
  );
  assert.equal(byName.key.required, true);
  assert.deepEqual(byName.mode.enumValues, ["ecb", "cbc", "ctr"]);
  assert.equal(byName.mode.defaultValue, "cbc");
});

test("real Argon2 schema: multiple integers with min/max", () => {
  const fields = extractFields({
    type: "object",
    required: ["password", "salt"],
    properties: {
      password: { type: "string", description: "Password (utf8)" },
      salt: { type: "string", description: "Salt (utf8, >=8 bytes recommended)" },
      variant: { type: "string", enum: ["argon2id", "argon2i", "argon2d"], default: "argon2id" },
      m_cost: { type: "integer", minimum: 8, maximum: 65536, default: 19456 },
      t_cost: { type: "integer", minimum: 1, maximum: 10, default: 2 },
      p_cost: { type: "integer", minimum: 1, maximum: 4, default: 1 },
      length: { type: "integer", minimum: 4, maximum: 128, default: 32 },
    },
  });
  const byName = Object.fromEntries(fields.map((f) => [f.name, f]));
  assert.equal(byName.m_cost.minimum, 8);
  assert.equal(byName.m_cost.maximum, 65536);
  assert.equal(byName.t_cost.defaultValue, 2);
  assert.equal(byName.length.minimum, 4);
  assert.deepEqual(byName.variant.enumValues, ["argon2id", "argon2i", "argon2d"]);
});

// ===== Validation tests =====

test("validateParams: valid flat schema passes", () => {
  const schema = {
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string", description: "Key" },
    },
  };
  const result = validateParams(schema, { key: "abc" });
  assert.equal(result.valid, true);
  assert.equal(result.errors.length, 0);
});

test("validateParams: missing required → error", () => {
  const schema = {
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string" },
    },
  };
  const result = validateParams(schema, {});
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.field === "key" && e.message.includes("required")));
});

test("validateParams: empty string for required → error", () => {
  const schema = {
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string" },
    },
  };
  const result = validateParams(schema, { key: "" });
  assert.equal(result.valid, false);
});

test("validateParams: string → integer error", () => {
  const schema = {
    type: "object",
    properties: {
      count: { type: "integer" },
    },
  };
  const result = validateParams(schema, { count: "not-a-number" });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.field === "count" && e.message.includes("integer")));
});

test("validateParams: number string → integer error", () => {
  const schema = {
    type: "object",
    properties: {
      count: { type: "integer" },
    },
  };
  const result = validateParams(schema, { count: 3.14 });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes("integer")));
});

test("validateParams: integer below minimum → error", () => {
  const schema = {
    type: "object",
    properties: {
      rounds: { type: "integer", minimum: 8, maximum: 65536 },
    },
  };
  const result = validateParams(schema, { rounds: 3 });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes(">=")));
});

test("validateParams: integer above maximum → error", () => {
  const schema = {
    type: "object",
    properties: {
      rounds: { type: "integer", minimum: 8, maximum: 65536 },
    },
  };
  const result = validateParams(schema, { rounds: 100000 });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes("<=")));
});

test("validateParams: enum value not in list → error", () => {
  const schema = {
    type: "object",
    properties: {
      mode: { type: "string", enum: ["ecb", "cbc", "ctr"] },
    },
  };
  const result = validateParams(schema, { mode: "invalid" });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes("must be one of")));
});

test("validateParams: boolean wrong type → error", () => {
  const schema = {
    type: "object",
    properties: {
      flag: { type: "boolean" },
    },
  };
  const result = validateParams(schema, { flag: "yes" });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes("boolean")));
});

test("validateParams: number type passes", () => {
  const schema = {
    type: "object",
    properties: {
      ratio: { type: "number", minimum: 0, maximum: 1 },
    },
  };
  const result = validateParams(schema, { ratio: 0.5 });
  assert.equal(result.valid, true);
});

test("validateParams: number below minimum → error", () => {
  const schema = {
    type: "object",
    properties: {
      ratio: { type: "number", minimum: 0, maximum: 1 },
    },
  };
  const result = validateParams(schema, { ratio: -0.1 });
  assert.equal(result.valid, false);
});

test("validateParams: valid enum passes", () => {
  const schema = {
    type: "object",
    properties: {
      mode: { type: "string", enum: ["ecb", "cbc", "ctr"] },
    },
  };
  const result = validateParams(schema, { mode: "cbc" });
  assert.equal(result.valid, true);
});

test("validateParams: object type wrong → error", () => {
  const schema = {
    type: "object",
    properties: {
      config: { type: "object" },
    },
  };
  const result = validateParams(schema, { config: "string-instead" });
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.message.includes("object")));
});

test("validateParams: no required, no params → valid", () => {
  const schema = {
    type: "object",
    properties: {
      optional: { type: "string" },
    },
  };
  const result = validateParams(schema, {});
  assert.equal(result.valid, true);
});

// ===== formStateToParams =====

test("formStateToParams: strips null and undefined", () => {
  const result = formStateToParams({
    key: "abc",
    mode: null,
    iv: undefined,
    count: 5,
  });
  assert.deepEqual(result, { key: "abc", count: 5 });
});

test("formStateToParams: preserves boolean and number", () => {
  const result = formStateToParams({
    flag: true,
    count: 42,
    name: "test",
  });
  assert.deepEqual(result, { flag: true, count: 42, name: "test" });
});

// ===== schemaToFormState =====

test("schemaToFormState: uses params over defaults", () => {
  const schema = {
    type: "object",
    properties: {
      mode: { type: "string", enum: ["a", "b"], default: "a" },
    },
  };
  const state = schemaToFormState(schema, { mode: "b" });
  assert.equal(state.mode, "b");
});

test("schemaToFormState: uses default when params missing", () => {
  const schema = {
    type: "object",
    properties: {
      mode: { type: "string", enum: ["a", "b"], default: "a" },
    },
  };
  const state = schemaToFormState(schema, {});
  assert.equal(state.mode, "a");
});

test("schemaToFormState: boolean → false empty", () => {
  const schema = {
    type: "object",
    properties: { flag: { type: "boolean" } },
  };
  const state = schemaToFormState(schema, {});
  assert.equal(state.flag, false);
});

test("schemaToFormState: integer → null empty", () => {
  const schema = {
    type: "object",
    properties: { count: { type: "integer" } },
  };
  const state = schemaToFormState(schema, {});
  assert.equal(state.count, null);
});

// ===== Roundtrip =====

test("roundtrip: schema → formState → params → validate passes", () => {
  const schema = {
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string", description: "Key" },
      mode: { type: "string", enum: ["ecb", "cbc"], default: "cbc" },
      rounds: { type: "integer", minimum: 1, maximum: 100, default: 10 },
    },
  };
  const state = schemaToFormState(schema, { key: "secret" });
  const params = formStateToParams(state);
  const result = validateParams(schema, params);
  assert.equal(result.valid, true);
  assert.equal(params.key, "secret");
  assert.equal(params.mode, "cbc");
  assert.equal(params.rounds, 10);
});

test("roundtrip: invalid value detected by validate", () => {
  const schema = {
    type: "object",
    required: ["key"],
    properties: {
      key: { type: "string" },
      rounds: { type: "integer", minimum: 1, maximum: 100 },
    },
  };
  const state = { key: "test", rounds: 200 };
  const params = formStateToParams(state);
  const result = validateParams(schema, params);
  assert.equal(result.valid, false);
  assert.ok(result.errors.some((e) => e.field === "rounds"));
});

// ===== Unsupported constructs =====

test("unsupported oneOf structure → empty enum", () => {
  const fields = extractFields({
    type: "object",
    properties: {
      mode: {
        oneOf: [{ title: "A" }, { title: "B" }], // no const
      },
    },
  });
  assert.deepEqual(fields[0].enumValues, []);
});

test("schema with no properties → empty fields, no crash", () => {
  const result = validateParams({ type: "object" }, { key: "val" });
  // No fields defined, so no validation errors (no required fields to check)
  assert.equal(result.valid, true);
});
