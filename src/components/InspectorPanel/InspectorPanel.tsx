import * as React from "react";
import {
  asParamsObject,
  extractFields,
  validateParams,
  type SchemaField,
  type ValidationError,
} from "@/lib/schemaForm";
import { useAppStore } from "@/store/useAppStore";

/**
 * InspectorPanel — авто-форма параметров из JSON Schema операции (FR-3.2:
 * "фронтенд рендерит форму параметров автоматически на основе схемы").
 * Поддерживаемое подмножество: string+enum → select, boolean → checkbox,
 * integer/number → number input, прочее string → text input. Неизвестные
 * поля схемы не рендерятся (не молча теряются — схема принадлежит
 * Rust-стороне и расширяется вместе с формой).
 */

export function InspectorPanel() {
  const node = useAppStore((s) => (s.selectedNodeId ? s.nodes[s.selectedNodeId] : undefined));
  const operation = useAppStore((s) =>
    node ? s.operations.find((o) => o.id === node.operationId) : undefined,
  );
  const updateNodeParams = useAppStore((s) => s.updateNodeParams);

  // Значение поля: params узла → default схемы → пусто.
  const fields = React.useMemo(
    () => (operation ? extractFields(operation.paramsSchema) : []),
    [operation],
  );

  if (!node || !operation) {
    return (
      <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
        <h2 className="text-sm font-medium text-text-secondary">Inspector</h2>
        <p className="mt-1 text-xs text-text-muted">
          {node
            ? `Схема операции ${node.operationId} недоступна — обновите реестр (⌘K).`
            : "Выберите узел в канвасе, чтобы редактировать параметры."}
        </p>
      </section>
    );
  }

  const params = asParamsObject(node.params);
  const validation = React.useMemo(
    () =>
      operation
        ? validateParams(operation.paramsSchema, params)
        : { valid: true, errors: [] as ValidationError[] },
    [operation, params],
  );
  const validationErrors = React.useMemo(() => {
    const map = new Map<string, string>();
    for (const err of validation.errors) map.set(err.field, err.message);
    return map;
  }, [validation]);

  const renderField = (field: SchemaField) => {
    const value = params[field.name];
    const effective = value !== undefined ? value : field.defaultValue;
    const fieldError = validationErrors.get(field.name);

    const labelText = field.required ? `${field.name} *` : field.name;

    const label = (
      <label
        htmlFor={`param-${field.name}`}
        className="text-2xs uppercase tracking-wide text-text-muted"
      >
        {labelText}
      </label>
    );

    const description = field.description ? (
      <span className="text-3xs text-text-muted/70">{field.description}</span>
    ) : null;

    const errorText = fieldError ? (
      <span className="text-3xs text-red-500">{fieldError}</span>
    ) : null;

    if (field.enumValues.length > 0) {
      const current = typeof effective === "string" ? effective : (field.enumValues[0] ?? "");
      return (
        <div key={field.name} className="flex flex-col gap-1">
          {label}
          <select
            id={`param-${field.name}`}
            data-selectable
            value={current}
            onChange={(e) => updateNodeParams(node.id, { [field.name]: e.target.value })}
            className={[
              "rounded-md border border-border-default bg-surface-2 px-2 py-1.5",
              "text-xs text-text-primary outline-none focus:border-border-focus",
            ].join(" ")}
          >
            {field.enumValues.map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
          {description}
          {errorText}
        </div>
      );
    }

    if (field.type === "boolean") {
      const checked = effective === true;
      return (
        <div key={field.name} className="flex items-center gap-2">
          <input
            id={`param-${field.name}`}
            type="checkbox"
            checked={checked}
            onChange={(e) => updateNodeParams(node.id, { [field.name]: e.target.checked })}
            className="h-3.5 w-3.5 accent-[var(--accent-9)]"
          />
          {label}
          {description}
          {errorText}
        </div>
      );
    }

    if (field.type === "integer" || field.type === "number") {
      const text = typeof effective === "number" ? String(effective) : "";
      return (
        <div key={field.name} className="flex flex-col gap-1">
          {label}
          <input
            id={`param-${field.name}`}
            data-selectable
            type="number"
            value={text}
            onChange={(e) => {
              const parsed =
                field.type === "integer"
                  ? Number.parseInt(e.target.value, 10)
                  : Number.parseFloat(e.target.value);
              updateNodeParams(node.id, {
                [field.name]: Number.isNaN(parsed) ? null : parsed,
              });
            }}
            className={[
              "rounded-md border border-border-default bg-surface-2 px-2 py-1.5",
              "font-mono text-xs text-text-primary outline-none focus:border-border-focus",
            ].join(" ")}
          />
          {description}
          {errorText}
        </div>
      );
    }

    const text = typeof effective === "string" ? effective : "";
    return (
      <div key={field.name} className="flex flex-col gap-1">
        {label}
        <input
          id={`param-${field.name}`}
          data-selectable
          type="text"
          value={text}
          onChange={(e) => updateNodeParams(node.id, { [field.name]: e.target.value })}
          className={[
            "rounded-md border border-border-default bg-surface-2 px-2 py-1.5",
            "font-mono text-xs text-text-primary outline-none focus:border-border-focus",
          ].join(" ")}
        />
        {description}
        {errorText}
      </div>
    );
  };

  return (
    <section className="rounded-lg border border-border-subtle bg-surface-1 p-4">
      <header className="mb-3 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-text-secondary">Inspector</h2>
        <span className="font-mono text-2xs text-text-muted">
          {node.operationId}@{node.operationVersion}
        </span>
      </header>
      {fields.length === 0 ? (
        <p className="text-xs text-text-muted">У операции нет параметров.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {fields.map(renderField)}
          {validation.errors.length > 0 && (
            <div className="mt-2 rounded-md border border-red-500/30 bg-red-500/10 p-2">
              <p className="text-xs text-red-400">
                {validation.errors.length} validation error(s):
              </p>
              <ul className="mt-1 list-inside list-disc text-3xs text-red-400/80">
                {validation.errors.map((err, i) => (
                  <li key={i}>{err.message}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
