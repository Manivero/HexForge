// Command palette: plugin operations are visibly badged, builtins untouched.
// Real import from .fe-build (compiled src), same pattern as plugins.test.mjs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildNavigationCommands, operationsToCommands, parseInlineArgs, stripInlineArgs, formatInlineArgs } from "../.fe-build/components/CommandPalette/commands.js";

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

// FR-2.5 navigation tests
test("buildNavigationCommands: creates 5 navigation commands", () => {
  const actions = {
    toggleInspector: () => {},
    toggleInput: () => {},
    toggleHistory: () => {},
    togglePlugins: () => {},
    togglePreview: () => {},
  };
  const cmds = buildNavigationCommands(actions);
  assert.equal(cmds.length, 5);
  assert.ok(cmds.every((c) => c.groupId === "navigation"));
});

test("buildNavigationCommands: each command has required fields", () => {
  const actions = {
    toggleInspector: () => {},
    toggleInput: () => {},
    toggleHistory: () => {},
    togglePlugins: () => {},
    togglePreview: () => {},
  };
  const cmds = buildNavigationCommands(actions);
  for (const cmd of cmds) {
    assert.ok(cmd.id.length > 0);
    assert.ok(cmd.label.length > 0);
    assert.ok(cmd.keywords && cmd.keywords.length > 0);
    assert.equal(typeof cmd.run, "function");
  }
});

test("buildNavigationCommands: toggleInspector calls action", () => {
  let called = false;
  const actions = {
    toggleInspector: () => { called = true; },
    toggleInput: () => {},
    toggleHistory: () => {},
    togglePlugins: () => {},
    togglePreview: () => {},
  };
  const cmds = buildNavigationCommands(actions);
  const inspector = cmds.find((c) => c.id === "nav.toggle-inspector");
  assert.ok(inspector);
  inspector?.run();
  assert.equal(called, true);
});

// FR-2.4 inline args tests

test("parseInlineArgs: extracts single key-value pair", () => {
  const args = parseInlineArgs("base64 decode / alphabet = url_safe");
  assert.deepEqual(args, { alphabet: "url_safe" });
});

test("parseInlineArgs: extracts multiple comma-separated pairs", () => {
  const args = parseInlineArgs("xor / key = abc, mode = cbc");
  assert.deepEqual(args, { key: "abc", mode: "cbc" });
});

test("parseInlineArgs: extracts semicolon-separated pairs", () => {
  const args = parseInlineArgs("gzip / level = 9; fast = true");
  assert.deepEqual(args, { level: "9", fast: "true" });
});

test("parseInlineArgs: returns empty object when no slash", () => {
  const args = parseInlineArgs("base64 decode");
  assert.deepEqual(args, {});
});

test("parseInlineArgs: returns empty object for trailing slash only", () => {
  const args = parseInlineArgs("base64 decode /");
  assert.deepEqual(args, {});
});

test("parseInlineArgs: trims whitespace around keys and values", () => {
  const args = parseInlineArgs("xor /   key   =   abc  ");
  assert.deepEqual(args, { key: "abc" });
});

test("parseInlineArgs: ignores segments without equals sign", () => {
  const args = parseInlineArgs("xor / key = abc, invalid_segment");
  assert.deepEqual(args, { key: "abc" });
});

test("stripInlineArgs: removes everything after slash", () => {
  assert.equal(stripInlineArgs("base64 decode / alphabet = url_safe"), "base64 decode");
});

test("stripInlineArgs: returns original when no slash", () => {
  assert.equal(stripInlineArgs("base64 decode"), "base64 decode");
});

test("stripInlineArgs: handles empty string", () => {
  assert.equal(stripInlineArgs(""), "");
});

test("formatInlineArgs: formats single arg", () => {
  assert.equal(formatInlineArgs({ key: "abc" }), "key = abc");
});

test("formatInlineArgs: formats multiple args", () => {
  const result = formatInlineArgs({ key: "abc", mode: "cbc" });
  // Order depends on Object.entries iteration
  assert.ok(result.includes("key = abc"));
  assert.ok(result.includes("mode = cbc"));
});

test("formatInlineArgs: empty args returns empty string", () => {
  assert.equal(formatInlineArgs({}), "");
});

test("roundtrip: parse then format preserves data", () => {
  const query = "xor / key = abc, mode = cbc";
  const args = parseInlineArgs(query);
  const formatted = formatInlineArgs(args);
  const reparsed = parseInlineArgs(`op / ${formatted}`);
  assert.deepEqual(reparsed, args);
});
