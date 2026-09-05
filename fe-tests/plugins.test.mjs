// Панель плагинов: чистые переходы (реальный импорт из .fe-build) +
// parity-проверки проводки store/panel/IPC по исходникам (паттерн history.test.mjs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  KNOWN_CAPABILITIES,
  activeCapabilities,
  applyGrantedCapability,
  applyRevokedCapability,
  isGranted,
  pendingCapabilities,
  pluginStatus,
} from "../.fe-build/plugins.js";

function dto(overrides = {}) {
  return {
    id: "plugin.example",
    displayName: "Example",
    name: "Example",
    version: "1.0.0",
    category: "Plugin",
    author: "HexForge",
    signatureValid: true,
    requestedCapabilities: ["filesystem_read", "network"],
    grantedCapabilities: [],
    ...overrides,
  };
}

test("pluginStatus различает valid / invalid-signature", () => {
  assert.equal(pluginStatus(dto()), "valid");
  assert.equal(pluginStatus(dto({ signatureValid: false })), "invalid-signature");
});

test("pending/active делят requested по факту гранта", () => {
  const p = dto({ grantedCapabilities: ["filesystem_read"] });
  assert.ok(isGranted(p, "filesystem_read"));
  assert.ok(!isGranted(p, "network"));
  assert.deepEqual(pendingCapabilities(p), ["network"]);
  assert.deepEqual(activeCapabilities(p), ["filesystem_read"]);
});

test("grant → revoke: полный IPC-цикл на чистых переходах", () => {
  const plugins = [dto()];
  // grant_capability вернул true → стор применяет applyGrantedCapability
  const granted = applyGrantedCapability(plugins, "plugin.example", "filesystem_read");
  assert.deepEqual(granted[0].grantedCapabilities, ["filesystem_read"]);
  assert.deepEqual(pendingCapabilities(granted[0]), ["network"]);
  // повторный грант идемпотентен (та же ссылка — стор не триггерит ререндер)
  assert.equal(applyGrantedCapability(granted, "plugin.example", "filesystem_read"), granted);
  // revoke_capability вернул true → стор применяет applyRevokedCapability
  const revoked = applyRevokedCapability(granted, "plugin.example", "filesystem_read");
  assert.deepEqual(revoked[0].grantedCapabilities, []);
  assert.equal(applyRevokedCapability(revoked, "plugin.example", "filesystem_read"), revoked);
});

test("переходы отклоняют чужое: неизвестные id/capability не меняют список", () => {
  const plugins = [dto()];
  assert.equal(applyGrantedCapability(plugins, "plugin.ghost", "filesystem_read"), plugins);
  assert.equal(
    applyGrantedCapability(plugins, "plugin.example", "sudo"),
    plugins,
  );
  assert.equal(applyRevokedCapability(plugins, "plugin.ghost", "filesystem_read"), plugins);
  assert.deepEqual(KNOWN_CAPABILITIES, ["filesystem_read", "filesystem_write", "network"]);
});

test("useAppStore: plugin-срез поверх существующих IPC-обёрток", () => {
  const src = readFileSync(new URL("../src/store/useAppStore.ts", import.meta.url), "utf-8");
  for (const sym of [
    "plugins",
    "pluginsLoading",
    "pluginsError",
    "loadPlugins",
    "installPluginFromPaths",
    "grantPluginCapability",
    "revokePluginCapability",
    "ipcListPlugins",
    "ipcInstallPlugin",
    "ipcGrantCapability",
    "ipcRevokeCapability",
    "applyGrantedCapability",
    "applyRevokedCapability",
    "loadOperations",
  ]) {
    assert.ok(src.includes(sym), `store must reference ${sym}`);
  }
  // Никакой дублированной валидации подписей/ABI в сторе.
  assert.ok(!src.includes("signatureValid ="), "store must not compute signatures");
  assert.ok(!src.includes("verify_signature"), "store must not verify signatures");
});

test("PluginPanel: список, подпись, capabilities, install/discovery", () => {
  const src = readFileSync(
    new URL("../src/components/PluginPanel/PluginPanel.tsx", import.meta.url),
    "utf-8",
  );
  for (const sym of [
    "displayName",
    "plugin.version",
    "plugin.category",
    "plugin.id",
    "signatureValid",
    "plugins.signatureInvalid",
    "pendingCapabilities",
    "activeCapabilities",
    "installPluginFromPaths",
    "grantPluginCapability",
    "revokePluginCapability",
    "loadPlugins",
    "plugins.loading",
    "plugins.empty",
    "plugins.unavailable",
    "plugins.install",
  ]) {
    assert.ok(src.includes(sym), `PluginPanel must reference ${sym}`);
  }
});

test("App: PluginPanel смонтирована в shell", () => {
  const src = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf-8");
  assert.ok(src.includes("PluginPanel"), "App must render PluginPanel");
});

test("ipc: install/grant/revoke вызывают свои Tauri-команды", () => {
  const src = readFileSync(new URL("../src/lib/ipc.ts", import.meta.url), "utf-8");
  assert.ok(src.includes(`"install_plugin"`), "installPlugin must call install_plugin");
  assert.ok(src.includes(`"grant_capability"`), "grantCapability must call grant_capability");
  assert.ok(src.includes(`"revoke_capability"`), "revokeCapability must call revoke_capability");
});

test("контракт: PluginManifestDto несёт displayName/category", () => {
  const src = readFileSync(new URL("../src/lib/ipc-contract.ts", import.meta.url), "utf-8");
  assert.ok(src.includes("displayName: string"), "contract must carry displayName");
  assert.ok(src.includes("category: string"), "contract must carry category");
});
