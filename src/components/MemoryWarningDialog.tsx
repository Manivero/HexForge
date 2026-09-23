import * as React from "react";
import { t } from "@/lib/i18n";

export interface MemoryWarningDialogProps {
  sizeMb: number;
  operationId: string;
  onCancel: () => void;
  onConfirm: () => void;
}

export function MemoryWarningDialog({
  sizeMb,
  operationId,
  onCancel,
  onConfirm,
}: MemoryWarningDialogProps) {
  const locale = React.useSyncExternalStore(
    () => () => {},
    () => localStorage.getItem("hexforge.locale") === "ru" ? "ru" : "en",
  );

  const isLarge = sizeMb >= 64;
  const message = isLarge
    ? t(locale, "app.memoryWarningLarge", { size: String(sizeMb) })
    : t(locale, "app.memoryWarning", { size: String(sizeMb) });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="w-full max-w-sm rounded-lg border border-border-subtle bg-surface-1 p-6 shadow-xl">
        <h3 className="text-sm font-medium text-text-primary">
          {operationId}
        </h3>
        <p className="mt-3 text-xs text-text-secondary">
          {message}
        </p>
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded border border-border-subtle px-3 py-1.5 text-xs text-text-muted hover:border-border-focus"
          >
            {t(locale, "app.cancel")}
          </button>
          <button
            onClick={onConfirm}
            className="rounded border border-status-error bg-status-error/10 px-3 py-1.5 text-xs text-status-error hover:bg-status-error/20"
          >
            {t(locale, "app.runAnyways")}
          </button>
        </div>
      </div>
    </div>
  );
}
