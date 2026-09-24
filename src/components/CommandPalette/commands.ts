import type { OperationDescriptor } from "../../lib/ipc-contract";

export type CommandGroupId = "app" | "operations" | "navigation";

export interface PaletteCommand {
  id: string;
  groupId: CommandGroupId;
  label: string;
  hint?: string;
  keywords?: string[];
  /** Inline args hint shown when query contains "/" (FR-2.4). */
  argsHint?: string;
  /** Callback receives optional parsed inline args. */
  run: (params?: Record<string, unknown>) => void | Promise<void>;
}

/** Static application commands (FR-2.5). */
export interface AppActions {
  toggleTheme: () => void;
  runGreetTest: () => void;
  clearGraph: () => void;
  deleteSelectedNode: () => boolean;
}

export function buildAppCommands(actions: AppActions): PaletteCommand[] {
  return [
    {
      id: "app.toggle-theme",
      groupId: "app",
      label: "Toggle Theme",
      hint: "Dark / Light",
      keywords: ["theme", "dark", "light", "appearance"],
      run: () => actions.toggleTheme(),
    },
    {
      id: "app.verify-bridge",
      groupId: "app",
      label: "Verify Rust Bridge (greet)",
      hint: "Sanity-check IPC",
      keywords: ["greet", "bridge", "ipc", "health", "ping"],
      run: () => actions.runGreetTest(),
    },
    {
      id: "app.clear-graph",
      groupId: "app",
      label: "Clear Graph",
      hint: "Remove all nodes",
      keywords: ["clear", "graph", "reset", "nodes"],
      run: () => actions.clearGraph(),
    },
    {
      id: "app.delete-selected",
      groupId: "app",
      label: "Delete Selected Node",
      hint: "Bridge children to parent",
      keywords: ["delete", "node", "remove", "selected"],
      run: () => actions.deleteSelectedNode(),
    },
  ];
}

/** Navigation commands for toggling UI panels (FR-2.5). */
export interface NavigationActions {
  toggleInspector: () => void;
  toggleInput: () => void;
  toggleHistory: () => void;
  togglePlugins: () => void;
  togglePreview: () => void;
}

export function buildNavigationCommands(actions: NavigationActions): PaletteCommand[] {
  return [
    {
      id: "nav.toggle-inspector",
      groupId: "navigation",
      label: "Toggle Inspector Panel",
      hint: "Show / hide operation parameters",
      keywords: ["inspector", "panel", "parameters", "params", "operation", "settings"],
      run: () => actions.toggleInspector(),
    },
    {
      id: "nav.toggle-input",
      groupId: "navigation",
      label: "Toggle Input Panel",
      hint: "Show / hide text input",
      keywords: ["input", "panel", "text", "source", "data", "entry"],
      run: () => actions.toggleInput(),
    },
    {
      id: "nav.toggle-history",
      groupId: "navigation",
      label: "Toggle History Panel",
      hint: "Show / hide time-travel timeline",
      keywords: ["history", "panel", "timeline", "snapshots", "time-travel", "undo"],
      run: () => actions.toggleHistory(),
    },
    {
      id: "nav.toggle-plugins",
      groupId: "navigation",
      label: "Toggle Plugins Panel",
      hint: "Show / hide installed plugins",
      keywords: ["plugins", "panel", "extensions", "modules", "installed"],
      run: () => actions.togglePlugins(),
    },
    {
      id: "nav.toggle-preview",
      groupId: "navigation",
      label: "Toggle Preview Dock",
      hint: "Show / hide hex/text preview",
      keywords: ["preview", "dock", "output", "hex", "text", "result", "bytes"],
      run: () => actions.togglePreview(),
    },
  ];
}

/** Maps operation descriptors to palette commands (FR-2.3). */
export function operationsToCommands(
  operations: OperationDescriptor[],
  onSelect: (operation: OperationDescriptor, params?: Record<string, unknown>) => void,
): PaletteCommand[] {
  return operations.map((op) => ({
    id: `op.${op.id}`,
    groupId: "operations",
    label: op.displayName,
    hint: op.origin === "plugin" ? `${op.category} • plugin` : op.category,
    keywords:
      op.origin === "plugin" ? [op.category, op.id, "plugin"] : [op.category, op.id],
    run: (params?: Record<string, unknown>) => onSelect(op, params),
  }));
}

/** Parses inline arguments from query using "/" syntax (FR-2.4).
 *
 * Examples:
 *   "base64 decode / alphabet = url_safe" → { alphabet: "url_safe" }
 *   "xor / key = abc"                     → { key: "abc" }
 *   "gzip / level = 9, fast = true"       → { level: "9", fast: "true" }
 *
 * Only string values are supported (numbers/booleans are coerced by backend).
 */
export function parseInlineArgs(query: string): Record<string, string> {
  const args: Record<string, string> = {};
  const slashIdx = query.indexOf("/");
  if (slashIdx === -1) return args;

  const argsPart = query.slice(slashIdx + 1).trim();
  if (argsPart.length === 0) return args;

  // Split by comma or semicolon, each segment is "key = value"
  const segments = argsPart.split(/[,;]/);
  for (const segment of segments) {
    const eqIdx = segment.indexOf("=");
    if (eqIdx === -1) continue;
    const key = segment.slice(0, eqIdx).trim();
    const value = segment.slice(eqIdx + 1).trim();
    if (key.length > 0 && value.length > 0) {
      args[key] = value;
    }
  }
  return args;
}

/** Strips inline arguments from query for fuzzy matching (FR-2.4).
 *
 * "base64 decode / alphabet = url_safe" → "base64 decode"
 */
export function stripInlineArgs(query: string): string {
  const slashIdx = query.indexOf("/");
  if (slashIdx === -1) return query;
  return query.slice(0, slashIdx).trim();
}

/** Formats parsed args back to inline syntax for display. */
export function formatInlineArgs(args: Record<string, string>): string {
  const parts = Object.entries(args).map(([k, v]) => `${k} = ${v}`);
  return parts.join(", ");
}
