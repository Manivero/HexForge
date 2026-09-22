// Чистый парсер подмножества JSON Schema для авто-формы параметров
// (FR-3.2) — выделяется из InspectorPanel для покрытия node:test.
//
// Поддерживаемый subset:
//   type: object → properties (flat или nested через recursion)
//   required: string[]
//   description: string
//   enum: string[] (+ default)
//   minimum / maximum: number (для integer/number)
//   oneOf / anyOf: array of { const: string } → enum-подобный выбор
//
// Неподдерживаемые конструкции → безопасный fallback (поле "other" или
// пустой enum), без создания некорректных значений.

export type FieldType = "string" | "boolean" | "integer" | "number" | "object" | "other";

export interface SchemaField {
  name: string;
  type: FieldType;
  description: string;
  required: boolean;
  enumValues: string[];
  hasDefault: boolean;
  defaultValue: unknown;
  minimum: number | null;
  maximum: number | null;
  /** Nested properties для type: "object". Пусто для примитивов. */
  children: SchemaField[];
}

export interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
}

export interface ValidationError {
  field: string;
  message: string;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function enumValuesFrom(def: Record<string, unknown>): string[] {
  if (Array.isArray(def.enum)) {
    return def.enum.filter((v): v is string => typeof v === "string");
  }
  // oneOf / anyOf: [{ const: "a" }, { const: "b" }]
  for (const key of ["oneOf", "anyOf"]) {
    const arr = def[key];
    if (Array.isArray(arr)) {
      const values: string[] = [];
      for (const item of arr) {
        const obj = asRecord(item);
        if (obj && typeof obj.const === "string") {
          values.push(obj.const);
        }
      }
      if (values.length > 0) return values;
    }
  }
  return [];
}

function minimumFrom(def: Record<string, unknown>): number | null {
  if (typeof def.minimum === "number") return def.minimum;
  return null;
}

function maximumFrom(def: Record<string, unknown>): number | null {
  if (typeof def.maximum === "number") return def.maximum;
  return null;
}

function typeFrom(def: Record<string, unknown>): FieldType {
  const t = def.type;
  if (t === "string" || t === "boolean" || t === "integer" || t === "number" || t === "object") {
    return t;
  }
  return "other";
}

// ---------------------------------------------------------------------------
// extractFields
// ---------------------------------------------------------------------------

/**
 * Извлекает поля верхнего уровня из JSON Schema операции.
 * Рекурсивно обрабатывает вложенные `object.properties`.
 * Не-объектные/нестандартные определения пропускаются без ошибки.
 */
export function extractFields(schema: unknown): SchemaField[] {
  const obj = asRecord(schema);
  if (!obj) return [];
  const props = obj.properties;
  const propObj = asRecord(props);
  if (!propObj) return [];

  const required: string[] = Array.isArray(obj.required)
    ? obj.required.filter((v): v is string => typeof v === "string")
    : [];
  const requiredSet = new Set(required);

  const fields: SchemaField[] = [];
  for (const [name, rawDef] of Object.entries(propObj)) {
    const def = asRecord(rawDef);
    if (!def) continue;

    const type = typeFrom(def);
    const children: SchemaField[] = [];

    if (type === "object") {
      children.push(...extractFields(def));
    }

    fields.push({
      name,
      type,
      description: typeof def.description === "string" ? def.description : "",
      required: requiredSet.has(name),
      enumValues: enumValuesFrom(def),
      hasDefault: "default" in def,
      defaultValue: def.default,
      minimum: minimumFrom(def),
      maximum: maximumFrom(def),
      children,
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

// ---------------------------------------------------------------------------
// Form state helpers
// ---------------------------------------------------------------------------

/**
 * Строит начальное form-state из schema + текущих params.
 * Для каждого поля берётся params[name] → schema default → type-appropriate empty.
 */
export function schemaToFormState(
  schema: unknown,
  params: unknown,
): Record<string, unknown> {
  const paramObj = asParamsObject(params);
  const fields = extractFields(schema);
  const state: Record<string, unknown> = {};

  for (const field of fields) {
    const fromParams = paramObj[field.name];
    if (fromParams !== undefined) {
      state[field.name] = fromParams;
    } else if (field.hasDefault) {
      state[field.name] = field.defaultValue;
    } else {
      switch (field.type) {
        case "boolean":
          state[field.name] = false;
          break;
        case "integer":
        case "number":
          state[field.name] = null;
          break;
        default:
          state[field.name] = "";
      }
    }
  }

  return state;
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

function validateField(
  field: SchemaField,
  value: unknown,
  errors: ValidationError[],
): void {
  if (field.required && (value === null || value === undefined || value === "")) {
    errors.push({ field: field.name, message: `Field '${field.name}' is required` });
    return;
  }

  if (value === null || value === undefined || value === "") return;

  switch (field.type) {
    case "string":
      if (typeof value !== "string") {
        errors.push({ field: field.name, message: `Field '${field.name}' must be a string` });
      } else if (field.enumValues.length > 0 && !field.enumValues.includes(value)) {
        errors.push({
          field: field.name,
          message: `Field '${field.name}' must be one of: ${field.enumValues.join(", ")}`,
        });
      }
      break;

    case "boolean":
      if (typeof value !== "boolean") {
        errors.push({ field: field.name, message: `Field '${field.name}' must be a boolean` });
      }
      break;

    case "integer": {
      if (typeof value !== "number" || !Number.isInteger(value)) {
        errors.push({ field: field.name, message: `Field '${field.name}' must be an integer` });
      } else {
        if (field.minimum !== null && value < field.minimum) {
          errors.push({
            field: field.name,
            message: `Field '${field.name}' must be >= ${field.minimum}`,
          });
        }
        if (field.maximum !== null && value > field.maximum) {
          errors.push({
            field: field.name,
            message: `Field '${field.name}' must be <= ${field.maximum}`,
          });
        }
      }
      break;
    }

    case "number": {
      if (typeof value !== "number") {
        errors.push({ field: field.name, message: `Field '${field.name}' must be a number` });
      } else {
        if (field.minimum !== null && value < field.minimum) {
          errors.push({
            field: field.name,
            message: `Field '${field.name}' must be >= ${field.minimum}`,
          });
        }
        if (field.maximum !== null && value > field.maximum) {
          errors.push({
            field: field.name,
            message: `Field '${field.name}' must be <= ${field.maximum}`,
          });
        }
      }
      break;
    }

    case "object": {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        errors.push({ field: field.name, message: `Field '${field.name}' must be an object` });
      } else {
        const childObj = value as Record<string, unknown>;
        for (const child of field.children) {
          validateField(child, childObj[child.name], errors);
        }
      }
      break;
    }

    default:
      break;
  }
}

/**
 * Validates params against the schema.
 * Returns structured errors for UI display.
 */
export function validateParams(schema: unknown, params: unknown): ValidationResult {
  const paramObj = asParamsObject(params);
  const fields = extractFields(schema);
  const errors: ValidationError[] = [];

  for (const field of fields) {
    validateField(field, paramObj[field.name], errors);
  }

  return { valid: errors.length === 0, errors };
}

/**
 * Converts form state (flat Record) to JSON params suitable for backend.
 * Strips null/undefined values, preserves types.
 */
export function formStateToParams(
  formState: Record<string, unknown>,
): Record<string, unknown> {
  const params: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(formState)) {
    if (value !== null && value !== undefined) {
      params[key] = value;
    }
  }
  return params;
}
