// Command palette: plugin operations are visibly badged, builtins untouched.
// Real import from .fe-build (compiled src), same pattern as plugins.test.mjs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { operationsToCommands } from "../.fe-build/components/CommandPalette/commands.js";

function descriptor(overrides = {}) {
  return {
    id: "encoding.base64.decode",
    version: "1.0.0",
    displayName: "Base64 Decode",
    category: "Encoding",
    paramsSchema: { type: "object" },
    capabilities: { deterministic: true, streamable: false, memoryCost: "full_buffer" },
    origin: "builtin",
    ...overrides,
  };
}

test("builtin operations keep the plain category hint", () => {
  const [cmd] = operationsToCommands([descriptor()], () => {});
  assert.equal(cmd.hint, "Encoding");
  assert.deepEqual(cmd.keywords, ["Encoding", "encoding.base64.decode"]);
});

test("plugin operations get a plugin badge and keyword", () => {
  const [cmd] = operationsToCommands(
    [
      descriptor({
        id: "plugin:example.wit-uppercase",
        displayName: "Example WIT Uppercase",
        category: "Plugin",
        origin: "plugin",
      }),
    ],
    () => {},
  );
  assert.equal(cmd.id, "op.plugin:example.wit-uppercase");
  assert.equal(cmd.hint, "Plugin • plugin");
  assert.ok(cmd.keywords.includes("plugin"));
  assert.equal(cmd.label, "Example WIT Uppercase");
});
