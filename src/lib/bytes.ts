// Хелперы отображения байтов для PreviewDock. Байты никогда не пересекают
// IPC-границу напрямую (контракт 05) — только base64 через preview_bytes,
// декодирование в Uint8Array делает decodeBase64Chunk из ipc.ts.

/** Байты в hex-дамп с пробелами: "de ad be ef". */
export function toHexDump(bytes: Uint8Array, limit = 4096): string {
  const end = Math.min(bytes.length, limit);
  return Array.from(bytes.subarray(0, end), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join(" ");
}

/** Байты в строку с lossy-декодированием UTF-8 (некорректные байты → U+FFFD). */
export function toLossyUtf8(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

export interface HexRow {
  /** Абсолютный адрес первого байта ряда. */
  addr: number;
  /** 16 hex-пар, разделённых пробелами (хвостовой ряд короче). */
  hex: string;
  /** Печатные ASCII-символы, непечатные → '·'. */
  ascii: string;
}

const HEX_ROW_WIDTH = 16;

function byteToAscii(b: number): string {
  return b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : "·";
}

/**
 * Классический hex-дамп по 16 байт на ряд: адрес | hex-пары | ASCII.
 * Используется постраничным HexViewer'ом (страница = 4 КиБ = 256 рядов —
 * объём DOM остаётся малым без виртуализации).
 */
export function buildHexRows(bytes: Uint8Array, baseOffset: number): HexRow[] {
  const rows: HexRow[] = [];
  for (let start = 0; start < bytes.length; start += HEX_ROW_WIDTH) {
    const slice = bytes.subarray(start, start + HEX_ROW_WIDTH);
    const pairs: string[] = [];
    let ascii = "";
    for (let i = 0; i < HEX_ROW_WIDTH; i += 1) {
      const byte = slice[i];
      if (byte === undefined) {
        // Хвостовой ряд: добиваем до ширины для ровной ASCII-колонки.
        pairs.push("  ");
        ascii += " ";
      } else {
        pairs.push(byte.toString(16).padStart(2, "0"));
        ascii += byteToAscii(byte);
        if (i === 7) {
          pairs.push(""); // визуальный разряд посередине
        }
      }
    }
    rows.push({
      addr: baseOffset + start,
      hex: pairs.join(" ").trimEnd(),
      ascii,
    });
  }
  return rows;
}

/** Адрес в каноничном 8-значном hex: `0x0000ABCD`. */
export function formatAddr(addr: number): string {
  return `0x${addr.toString(16).padStart(8, "0")}`;
}

/**
 * Парсит строку hex-пар ("deadbeef", "DE AD" — пробелы игнорируются) в байты.
 *
 * @returns байты, либо null при нечётной длине / недопустимом символе /
 *          пустой строке.
 */
export function hexPairsToBytes(s: string): Uint8Array | null {
  const clean = s.replace(/\s+/g, "");
  if (clean.length === 0 || clean.length % 2 !== 0) return null;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    const pair = clean.slice(i * 2, i * 2 + 2);
    const value = Number.parseInt(pair, 16);
    if (Number.isNaN(value)) return null;
    out[i] = value;
  }
  return out;
}
