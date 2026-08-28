// Minimal i18n for HexForge — FR NFR-8: architectural readiness, en/ru.
// Keys are flat dot-notation; missing key falls back to key itself.

export type Locale = "en" | "ru";

export const translations: Record<Locale, Record<string, string>> = {
  en: {
    "app.emptyGraph": "Empty graph — start with ⌘K",
    "app.nodes": "{count} node(s)",
    "app.selected": "selected: {id}",
    "app.addOperation": "Add operation",
    "app.cancel": "Cancel",
    "app.runNode": "Run node",
    "app.running": "Running…",
    "app.sourceReady": "source ready",
    "app.noSource": "no source",
    "app.snapshots": "{count} snapshot(s)",
    "app.version": "HexForge v0.1.0 — Node Graph MVP",
    "app.selectNodeForRun": "Select a node to run (⌘K → operation)",
    "app.export": "Export",
    "app.import": "Import",
    "app.exportPrompt": "Enter export path (e.g. recipe.hexforge)",
    "app.importPrompt": "Enter import path",
    "app.missingOps": "Missing operations: {ops}",
    "input.placeholder": "Enter text or drop file…",
    "history.title": "History",
  },
  ru: {
    "app.emptyGraph": "Пустой граф — начните с ⌘K",
    "app.nodes": "{count} node(s)",
    "app.selected": "selected: {id}",
    "app.addOperation": "Add operation",
    "app.cancel": "Cancel",
    "app.runNode": "Run node",
    "app.running": "Running…",
    "app.sourceReady": "source ready",
    "app.noSource": "no source",
    "app.snapshots": "{count} snapshot(s)",
    "app.version": "HexForge v0.1.0 — Node Graph MVP",
    "app.selectNodeForRun": "Выберите узел для запуска (⌘K → операция)",
    "app.export": "Экспорт",
    "app.import": "Импорт",
    "app.exportPrompt": "Введите путь для экспорта (напр. recipe.hexforge)",
    "app.importPrompt": "Введите путь для импорта",
    "app.missingOps": "Отсутствуют операции: {ops}",
    "input.placeholder": "Введите текст или перетащите файл…",
    "history.title": "История",
  },
};

export function t(locale: Locale, key: string, params?: Record<string, string | number>): string {
  const dict = translations[locale] ?? translations.en;
  let tmpl = dict[key] ?? translations.en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      tmpl = tmpl.replaceAll(`{${k}}`, String(v));
    }
  }
  return tmpl;
}
