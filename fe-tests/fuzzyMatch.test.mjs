// Юнит-тесты fuzzyMatch — критичного ранжирования Command Palette (⌘K).
// Тестируется СКОМПИЛИРОВАННЫЙ артефакт (.fe-build), поэтому файл — .mjs
// и не требует дополнительных dev-зависимостей (node:test из stdlib).
import { test } from "node:test";
import assert from "node:assert/strict";
import { fuzzyMatch } from "../.fe-build/fuzzyMatch.js";

test("пустой запрос матчит всё с нулевым счётом", () => {
  const r = fuzzyMatch("", "Base64 Encode");
  assert.equal(r.matched, true);
  assert.equal(r.score, 0);
});

test("подпоследовательность через разрывы матчится", () => {
  // b64 → "Base64 Encode": B..6..4 с разрывами.
  const r = fuzzyMatch("b64", "Base64 Encode");
  assert.equal(r.matched, true);
  assert.ok(r.score > 0);
});

test("чужие буквы — нет совпадения", () => {
  assert.equal(fuzzyMatch("gzip", "Base64 Encode").matched, false);
});

test("регистронезависимость", () => {
  assert.equal(fuzzyMatch("BASE", "base64 encode").matched, true);
  assert.equal(fuzzyMatch("base", "BASE64 ENCODE").matched, true);
});

test("реальный запрос палитры находит операцию по подстроке названия", () => {
  // Ключевой UX-кейс ⌘K: пользователь печатает часть display_name операции.
  const hit = fuzzyMatch("base64 dec", "Base64 Decode");
  assert.equal(hit.matched, true);
});

test("пустая цель: непустой запрос не матчится", () => {
  assert.equal(fuzzyMatch("a", "").matched, false);
});
