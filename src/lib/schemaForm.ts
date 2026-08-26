// Чистый парсер подмножества JSON Schema для авто-формы параметров
// (FR-3.2) — выделяется из InspectorPanel для покрытия node:test.

export type FieldType = "string" | "boolean" | "integer" | "number" | "other";

export interface SchemaField {
  name: string;
  type: FieldType;
  enumValues: string[];
  hasDefault: boolean;
  defaultValue: unknown;
}

/**
 * Извлекает поля верхнего уровня из JSON Schema операции.
 * Поддерживаемое подмножество (см. InspectorPanel): string, boolean,
 * integer, number, string+enum; enum фильтруется до строк.
 * Не-объектные/нестандартные определения пропускаются без ошибки —
 * форма рендерит то, что понимает, остальное не теряется на бэкенде.
 */
export function extractFields(schema: unknown): SchemaField[] {
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
    const rawType = def.type;
    const type =
      rawType === "string" ||
      rawType === "boolean" ||
      rawType === "integer" ||
      rawType === "number"
        ? rawType
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

/** Приводит params узла к плоскому объекту (контракт допускает unknown). */
export function asParamsObject(params: unknown): Record<string, unknown> {
  return params !== null && typeof params === "object" && !Array.isArray(params)
    ? (params as Record<string, unknown>)
    : {};
}
