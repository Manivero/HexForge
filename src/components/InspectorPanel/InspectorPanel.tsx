import * as React from "react";
import { useAppStore } from "@/store/useAppStore";

/**
 * InspectorPanel — авто-форма параметров из JSON Schema операции (FR-3.2:
 * "фронтенд рендерит форму параметров автоматически на основе схемы").
 * Поддерживаемое подмножество: string+enum → select, boolean → checkbox,
 * integer/number → number input, прочее string → text input. Неизвестные
 * поля схемы не рендерятся (не молча теряются — схема принадлежит
 * Rust-стороне и расширяется вместе с формой).
 */

interface SchemaField {
  name: string;
  type: "string" | "boolean" | "integer" | "number" | "other";
  enumValues: string[];
  hasDefault: boolean;
  defaultValue: unknown;
}

function extractFields(schema: unknown): SchemaField[] {
  if (schema === null || typeof schema !== "object") return [];
  const obj = schema as Record<string, unknown>;
  const props = obj.properties;
  if (props === null || typeof props !== "object" || Array.isArray(props)) {
    return [];
  }
  const fields: SchemaField[] = [];
  for (const [name, rawDef] of Object.entries(props)) {
    if (rawDef === null || typeof rawDef !== "object" || Array.isArray(rawDef)) {
      continue;
    }
    const def = rawDef as Record<string, unknown>;
    const type =
      def.type === "string" ||
      def.type === "boolean" ||
      def.type === "integer" ||
      def.type === "number"
        ? def.type
        : "other";
    fields.push({
      name,
      type,
      enumValues: Array.isArray(def.enum)
        ? def.enum.filter((v): v is string => typeof v === "string")
        : [],
      hasDefault: "default" in def,
      defaultValue: def.default,
    });
  }
  return fields;
}

function asParamsObject(params: unknown): Record<string, unknown> {
  return params !== null &&
    typeof params === "object" &&
    !Array.isArray(params)
    ? (params as Record<string, unknown>)
    : {};
}

export function InspectorPanel() {
  const node = useAppStore((s) =>
    s.selectedNodeId ? s.nodes[s.selectedNodeId] : undefined,
  );
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

  const renderField = (field: SchemaField) => {
    const value = params[field.name];
    const effective = value !== undefined ? value : field.defaultValue;

    const label = (
      <label
        htmlFor={`param-${field.name}`}
        className="text-2xs uppercase tracking-wide text-text-muted"
      >
        {field.name}
      </label>
    );

    if (field.enumValues.length > 0) {
      const current =
        typeof effective === "string" ? effective : field.enumValues[0] ?? "";
      return (
        <div key={field.name} className="flex flex-col gap-1">
          {label}
          <select
            id={`param-${field.name}`}
            data-selectable
            value={current}
            onChange={(e) =>
              updateNodeParams(node.id, { [field.name]: e.target.value })
            }
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
            onChange={(e) =>
              updateNodeParams(node.id, { [field.name]: e.target.checked })
            }
            className="h-3.5 w-3.5 accent-[var(--accent-9)]"
          />
          {label}
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
          onChange={(e) =>
            updateNodeParams(node.id, { [field.name]: e.target.value })
          }
          className={[
            "rounded-md border border-border-default bg-surface-2 px-2 py-1.5",
            "font-mono text-xs text-text-primary outline-none focus:border-border-focus",
          ].join(" ")}
        />
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
        <p className="text-xs text-text-muted">
          У операции нет параметров.
        </p>
      ) : (
        <div className="flex flex-col gap-3">{fields.map(renderField)}</div>
      )}
    </section>
  );
}
