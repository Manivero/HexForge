import * as React from "react";
import { useAppStore } from "@/store/useAppStore";

/**
 * InputPanel — создание литерального источника байтов (первая половина
 * сквозного потока 05-IPC-CONTRACT.md §3). Лимит 16МБ гарантируется
 * серверной стороной create_literal_source; здесь только UX-ограничение
 * длины textarea для отзывчивости.
 */
export function InputPanel() {
  const [text, setText] = React.useState("HexForge > CyberChef");
  const sourceHandle = useAppStore((s) => s.sourceHandle);
  const sourceSizeBytes = useAppStore((s) => s.sourceSizeBytes);
  const creatingSource = useAppStore((s) => s.creatingSource);
  const createSource = useAppStore((s) => s.createSource);

  const disabled = creatingSource || text.length === 0;

  return (
    <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
      <header className="mb-2 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-text-secondary">Input data</h2>
        <span className="text-2xs text-text-muted">
          {sourceHandle
            ? `source ${sourceHandle.slice(0, 8)} · ${sourceSizeBytes ?? 0} B`
            : "no source"}
        </span>
      </header>
      <textarea
        data-selectable
        value={text}
        onChange={(e) => setText(e.target.value)}
        rows={3}
        spellCheck={false}
        placeholder="Текст будет передан в ядро как литеральный источник (UTF-8)"
        className={[
          "w-full resize-y rounded-md border border-border-default bg-surface-2 px-3 py-2",
          "font-mono text-xs text-text-primary outline-none",
          "placeholder:text-text-muted focus:border-border-focus",
        ].join(" ")}
      />
      <button
        onClick={() => void createSource(text)}
        disabled={disabled}
        className={[
          "mt-2 rounded-md border border-border-default bg-surface-2 px-3 py-1.5 text-xs",
          "text-text-primary transition-colors duration-fast ease-out-expo",
          "enabled:hover:border-border-focus enabled:hover:text-accent",
          "disabled:cursor-not-allowed disabled:opacity-50",
        ].join(" ")}
      >
        {creatingSource ? "Creating…" : "Create literal source"}
      </button>
    </section>
  );
}
