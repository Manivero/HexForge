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
