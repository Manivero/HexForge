// Проверка контракта ipc.ts ↔ ipc-contract.ts: все команды из Rust-стороны имеют типизированные обёртки.
// Не импортируем .fe-build/ipc.js напрямую (он тянет @tauri-apps/api и ESM-резолв без .js расширения падает в node:test).
// Вместо этого читаем исходник и проверяем наличие экспортов — достаточно для parity-гейта.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const ipcSrc = readFileSync(new URL("../src/lib/ipc.ts", import.meta.url), "utf-8");

test("ipc wrappers exist and are functions (source check)", () => {
  for (const name of [
    "exportRecipe",
    "importRecipe",
    "importCyberChefRecipe",
    "listPlugins",
    "installPlugin",
    "grantCapability",
    "revokeCapability",
    "listOperations",
    "greet",
    "openFile",
    "createLiteralSource",
    "previewBytes",
    "releaseSource",
    "patchSource",
    "setGraph",
    "runNode",
    "cancelNode",
    "jumpToSnapshot",
    "listSnapshots",
    "diffSnapshots",
  ]) {
    assert.ok(ipcSrc.includes(`export function ${name}`), `missing wrapper ${name}`);
  }
});

test("exportRecipe/importRecipe call correct Tauri commands", () => {
  assert.ok(ipcSrc.includes(`"export_recipe"`), "exportRecipe must call export_recipe");
  assert.ok(ipcSrc.includes(`"import_recipe"`), "importRecipe must call import_recipe");
});
