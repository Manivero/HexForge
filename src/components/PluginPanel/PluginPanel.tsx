import * as React from "react";
import { useAppStore } from "@/store/useAppStore";
import { t } from "@/lib/i18n";
import type { PluginCapability, PluginManifestDto } from "@/lib/ipc-contract";
import { activeCapabilities, pendingCapabilities, pluginStatus } from "@/lib/plugins";

/**
 * PluginPanel — список обнаруженных плагинов (FR-6) поверх починенного
 * `list_plugins`: id/version/displayName/category, статус подписи,
 * requested/granted capabilities, install через `install_plugin`,
 * grant/revoke через существующий IPC. Вся валидация — на бэкенде;
 * панель только отображает готовый PluginManifestDto.
 */

function CapabilityChips({
  plugin,
  onGrant,
  onRevoke,
  locale,
}: {
  plugin: PluginManifestDto;
  onGrant: (cap: PluginCapability) => void;
  onRevoke: (cap: PluginCapability) => void;
  locale: "en" | "ru";
}) {
  const pending = pendingCapabilities(plugin);
  const active = activeCapabilities(plugin);
  if (pending.length === 0 && active.length === 0) {
    return <span className="text-2xs text-text-muted">—</span>;
  }
  return (
    <span className="flex flex-wrap gap-1">
      {active.map((cap) => (
        <span
          key={`g-${cap}`}
          className="inline-flex items-center gap-1 rounded border border-accent px-1.5 py-0.5 text-2xs text-accent"
          title={t(locale, "plugins.granted")}
        >
          {cap}
          <button
            onClick={() => onRevoke(cap)}
            className="underline hover:text-text-primary"
            aria-label={`revoke ${cap} for ${plugin.id}`}
          >
            {t(locale, "plugins.revoke")}
          </button>
        </span>
      ))}
      {pending.map((cap) => (
        <span
          key={`r-${cap}`}
          className="inline-flex items-center gap-1 rounded border border-border-default px-1.5 py-0.5 text-2xs text-text-muted"
          title={t(locale, "plugins.requested")}
        >
          {cap}
          <button
            onClick={() => onGrant(cap)}
            className="underline hover:text-accent"
            aria-label={`grant ${cap} for ${plugin.id}`}
          >
            {t(locale, "plugins.grant")}
          </button>
        </span>
      ))}
    </span>
  );
}

export function PluginPanel() {
  const locale = useAppStore((s) => s.locale);
  const plugins = useAppStore((s) => s.plugins);
  const pluginsLoading = useAppStore((s) => s.pluginsLoading);
  const pluginsError = useAppStore((s) => s.pluginsError);
  const loadPlugins = useAppStore((s) => s.loadPlugins);
  const installPluginFromPaths = useAppStore((s) => s.installPluginFromPaths);
  const grantPluginCapability = useAppStore((s) => s.grantPluginCapability);
  const revokePluginCapability = useAppStore((s) => s.revokePluginCapability);

  const [wasmPath, setWasmPath] = React.useState("");
  const [manifestPath, setManifestPath] = React.useState("");

  React.useEffect(() => {
    void loadPlugins();
  }, [loadPlugins]);

  const handleInstall = React.useCallback(() => {
    if (!wasmPath.trim() || !manifestPath.trim()) return;
    void installPluginFromPaths(wasmPath.trim(), manifestPath.trim());
  }, [wasmPath, manifestPath, installPluginFromPaths]);

  return (
    <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
      <header className="mb-2 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-text-secondary">{t(locale, "plugins.title")}</h2>
        <button
          onClick={() => void loadPlugins()}
          disabled={pluginsLoading}
          className="rounded border border-border-subtle px-2 py-0.5 text-2xs hover:border-border-focus hover:text-accent disabled:cursor-not-allowed disabled:opacity-50"
        >
          {t(locale, "plugins.reload")}
        </button>
      </header>

      {pluginsLoading && plugins.length === 0 ? (
        <p className="text-xs text-text-muted">{t(locale, "plugins.loading")}</p>
      ) : pluginsError && plugins.length === 0 ? (
        <p className="text-xs text-status-error" data-selectable>
          {pluginsError}
          <span className="mt-1 block text-2xs text-text-muted">
            {t(locale, "plugins.unavailable")}
          </span>
        </p>
      ) : plugins.length === 0 ? (
        <p className="text-xs text-text-muted">{t(locale, "plugins.empty")}</p>
      ) : (
        <ol className="flex max-h-56 flex-col gap-1.5 overflow-y-auto">
          {plugins.map((plugin) => {
            const status = pluginStatus(plugin);
            const invalid = status === "invalid-signature";
            return (
              <li
                key={plugin.id}
                className={[
                  "rounded-md border px-2 py-1.5",
                  invalid ? "border-status-error" : "border-border-default",
                ].join(" ")}
              >
                <div className="flex flex-wrap items-baseline justify-between gap-1">
                  <span className="text-xs font-medium text-text-primary">
                    {plugin.displayName}
                    <span className="ml-2 font-mono text-2xs text-text-muted">{plugin.id}</span>
                  </span>
                  <span className="font-mono text-2xs text-text-muted">
                    v{plugin.version} · {plugin.category}
                  </span>
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-2">
                  <span
                    className={[
                      "rounded px-1.5 py-0.5 font-mono text-2xs",
                      invalid
                        ? "bg-status-error text-text-primary"
                        : "bg-surface-2 text-text-muted",
                    ].join(" ")}
                    title={plugin.author}
                  >
                    {invalid
                      ? t(locale, "plugins.signatureInvalid")
                      : t(locale, "plugins.signatureValid")}
                  </span>
                  <CapabilityChips
                    plugin={plugin}
                    locale={locale}
                    onGrant={(cap) => void grantPluginCapability(plugin.id, cap)}
                    onRevoke={(cap) => void revokePluginCapability(plugin.id, cap)}
                  />
                </div>
              </li>
            );
          })}
        </ol>
      )}

      {pluginsError && plugins.length > 0 && (
        <p className="mt-2 font-mono text-2xs text-status-error" data-selectable>
          {pluginsError}
        </p>
      )}

      <div className="mt-3 border-t border-border-subtle pt-2">
        <h3 className="mb-1 text-xs text-text-secondary">{t(locale, "plugins.installTitle")}</h3>
        <div className="flex flex-col gap-1.5">
          <input
            value={wasmPath}
            onChange={(e) => setWasmPath(e.target.value)}
            placeholder={t(locale, "plugins.wasmPath")}
            spellCheck={false}
            className="rounded border border-border-default bg-surface-0 px-2 py-1 font-mono text-xs text-text-primary placeholder:text-text-muted"
            aria-label="wasm path"
          />
          <input
            value={manifestPath}
            onChange={(e) => setManifestPath(e.target.value)}
            placeholder={t(locale, "plugins.manifestPath")}
            spellCheck={false}
            className="rounded border border-border-default bg-surface-0 px-2 py-1 font-mono text-xs text-text-primary placeholder:text-text-muted"
            aria-label="manifest path"
          />
          <div className="flex items-center gap-2">
            <button
              onClick={handleInstall}
              disabled={pluginsLoading || !wasmPath.trim() || !manifestPath.trim()}
              className="rounded-md border border-border-default bg-surface-1 px-3 py-1.5 text-xs text-text-primary enabled:hover:border-border-focus enabled:hover:text-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t(locale, "plugins.install")}
            </button>
            <span className="text-2xs text-text-muted">{t(locale, "plugins.sessionNote")}</span>
          </div>
        </div>
      </div>
    </section>
  );
}
