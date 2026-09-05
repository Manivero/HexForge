// Чистые хелперы панели плагинов — никакой логики бэкенда здесь нет:
// displayName/category/signatureValid/capabilities приходят готовыми из
// PluginManifestDto (commands::plugin_manifest_dto — единственный источник).
// Файл намеренно без импортов Tauri/React, чтобы выполняться в FE CI
// (node:test поверх .fe-build) как есть.

import type { PluginCapability, PluginManifestDto, PluginId } from "./ipc-contract";

export const KNOWN_CAPABILITIES: readonly PluginCapability[] = [
  "filesystem_read",
  "filesystem_write",
  "network",
];

/** Состояния строки плагина, которые обязан различимо показать UI. */
export type PluginStatusKind = "valid" | "invalid-signature";

export function pluginStatus(plugin: PluginManifestDto): PluginStatusKind {
  return plugin.signatureValid ? "valid" : "invalid-signature";
}

export function isGranted(plugin: PluginManifestDto, capability: PluginCapability): boolean {
  return plugin.grantedCapabilities.includes(capability);
}

/** Запрошенные, но ещё не выданные привилегии — кандидаты на Grant. */
export function pendingCapabilities(plugin: PluginManifestDto): PluginCapability[] {
  return plugin.requestedCapabilities.filter((c) => !plugin.grantedCapabilities.includes(c));
}

/** Выданные привилегии — кандидаты на Revoke. */
export function activeCapabilities(plugin: PluginManifestDto): PluginCapability[] {
  return plugin.grantedCapabilities.filter((c) =>
    (plugin.requestedCapabilities as string[]).includes(c),
  );
}

function withGrant(plugin: PluginManifestDto, capability: PluginCapability): PluginManifestDto {
  if (plugin.grantedCapabilities.includes(capability)) return plugin;
  return { ...plugin, grantedCapabilities: [...plugin.grantedCapabilities, capability] };
}

function withRevoke(plugin: PluginManifestDto, capability: PluginCapability): PluginManifestDto {
  if (!plugin.grantedCapabilities.includes(capability)) return plugin;
  return {
    ...plugin,
    grantedCapabilities: plugin.grantedCapabilities.filter((c) => c !== capability),
  };
}

/**
 * Оптимистичное применение подтверждённого бэкендом гранта
 * (`grant_capability` вернул true): чистая замена элемента списка.
 * Невалидная capability или неизвестный id — список без изменений.
 */
export function applyGrantedCapability(
  plugins: PluginManifestDto[],
  pluginId: PluginId,
  capability: PluginCapability,
): PluginManifestDto[] {
  if (!(KNOWN_CAPABILITIES as readonly string[]).includes(capability)) return plugins;
  let changed = false;
  const next = plugins.map((p) => {
    if (p.id !== pluginId) return p;
    const updated = withGrant(p, capability);
    if (updated !== p) changed = true;
    return updated;
  });
  return changed ? next : plugins;
}

/** Зеркально `applyGrantedCapability` для подтверждённого revoke. */
export function applyRevokedCapability(
  plugins: PluginManifestDto[],
  pluginId: PluginId,
  capability: PluginCapability,
): PluginManifestDto[] {
  if (!(KNOWN_CAPABILITIES as readonly string[]).includes(capability)) return plugins;
  let changed = false;
  const next = plugins.map((p) => {
    if (p.id !== pluginId) return p;
    const updated = withRevoke(p, capability);
    if (updated !== p) changed = true;
    return updated;
  });
  return changed ? next : plugins;
}
