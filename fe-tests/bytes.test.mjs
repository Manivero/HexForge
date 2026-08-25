// Юнит-тесты форматирования hex-viewer'а (buildHexRows/formatAddr) и
// lossy-декодирования — скомпилированные артефакты .fe-build.
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildHexRows, formatAddr, toLossyUtf8, toHexDump } from "../.fe-build/bytes.js";

test("короткий вход: один ряд с паддингом до 16 байт", () => {
  const rows = buildHexRows(new Uint8Array([0xde, 0xad]), 0);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].addr, 0);
  // "de ad" + 14 пустых пар + разряд посередине → trimEnd обрезает хвост,
  // но средний разряд даёт лишний пробел в середине — проверяем префикс.
  assert.ok(rows[0].hex.startsWith("de ad"));
  assert.equal(rows[0].ascii.length, 16);
  assert.ok(rows[0].ascii.startsWith("··"));
});

test("полный ряд: печатные ASCII и разряд после 8-го байта", () => {
  const bytes = new Uint8Array(16);
  for (let i = 0; i < 16; i += 1) bytes[i] = 0x41 + i; // A..P
  const [row] = buildHexRows(bytes, 0x100);
  assert.equal(row.addr, 0x100);
  assert.ok(row.hex.includes("41 42 43 44 45 46 47 48"), "первая восьмёрка");
  assert.ok(row.hex.includes("49 4a 4b 4c 4d 4e 4f 50"), "вторая восьмёрка");
  // Разряд: между '48' и '49' ровно два пробела (конец пары + разряд).
  assert.ok(row.hex.includes("48  49"));
  assert.equal(row.ascii, "ABCDEFGHIJKLMNOP");
});

test("непечатные байты → '·', пробел печатается как пробел", () => {
  const [row] = buildHexRows(new Uint8Array([0x00, 0xff, 0x7f, 0x20]), 0);
  // 0x00 и 0xff непечатны; 0x7f тоже вне диапазона 0x20..=0x7e; 0x20 — пробел.
  assert.equal(row.ascii.length, 16);
  assert.ok(row.ascii.startsWith("··· "));
});

test("несколько рядов: адреса идут с шагом 16 от baseOffset", () => {
  const bytes = new Uint8Array(33); // 3 ряда: 16+16+1
  const rows = buildHexRows(bytes, 4096);
  assert.equal(rows.length, 3);
  assert.deepEqual(
    rows.map((r) => r.addr),
    [4096, 4112, 4128],
  );
});

test("пустой вход — пустой список рядов", () => {
  assert.deepEqual(buildHexRows(new Uint8Array(), 0), []);
});

test("formatAddr: 8 значных цифр с префиксом 0x", () => {
  assert.equal(formatAddr(0), "0x00000000");
  assert.equal(formatAddr(0xabcd), "0x0000abcd");
  assert.equal(formatAddr(0x12345678), "0x12345678");
});

test("toLossyUtf8: некорректная последовательность → U+FFFD", () => {
  const out = toLossyUtf8(new Uint8Array([0x48, 0xff, 0x69])); // H <?> i
  assert.equal(out, "H\uFFFDi");
});

test("toHexDump: пары через пробел, limit обрезает", () => {
  assert.equal(toHexDump(new Uint8Array([0x01, 0x02]), 10), "01 02");
  assert.equal(toHexDump(new Uint8Array([1, 2, 3, 4]), 2), "01 02");
});
