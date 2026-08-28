//! `protobuf.decode_raw` — обход Protobuf wire-format без схемы
//! (PRD §3.3 "Protobuf без схемы — raw varint walk").
//!
//! Читает varint-теги, различает wire types (varint, 64-bit, length-delimited,
//! 32-bit), выводит человекочитаемое текстовое представление каждого поля.
//! Вложенные length-delimited поля дополнительно пробуются как вложенные
//! сообщения — если парсинг успешен, выводится рекурсивно с отступом.

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    Fixed32 = 5,
}

impl std::fmt::Display for WireType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl WireType {
    fn name(self) -> &'static str {
        match self {
            Self::Varint => "varint",
            Self::Fixed64 => "fixed64",
            Self::LengthDelimited => "length-delimited",
            Self::Fixed32 => "fixed32",
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_varint(&mut self) -> Result<u64, String> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            let b = *self
                .data
                .get(self.pos)
                .ok_or_else(|| format!("truncated varint at byte {}", self.pos))?;
            self.pos += 1;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(format!("varint too long at byte {}", self.pos));
            }
        }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.pos + len > self.data.len() {
            return Err(format!(
                "truncated field at byte {}: need {len} bytes",
                self.pos
            ));
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }
}

fn read_tag(reader: &mut Reader) -> Result<(u32, WireType), String> {
    let tag = reader.read_varint()?;
    let field_number = (tag >> 3) as u32;
    let wire_type_raw = (tag & 0x07) as u8;
    let wire_type = match wire_type_raw {
        0 => WireType::Varint,
        1 => WireType::Fixed64,
        2 => WireType::LengthDelimited,
        5 => WireType::Fixed32,
        _ => {
            return Err(format!(
                "unsupported wire type {wire_type_raw} at byte {}",
                reader.pos
            ))
        }
    };
    Ok((field_number, wire_type))
}

/// Пытается распарсить `data` как вложенное protobuf-сообщение.
/// Возвращает `None` при любой ошибке — это не ошибка данных, а признак того,
/// что байты не являются валидным вложенным сообщением.
fn try_parse_nested(data: &[u8]) -> Option<Vec<String>> {
    let mut reader = Reader::new(data);
    let mut lines = Vec::new();
    while reader.pos < data.len() {
        let (field, wt) = read_tag(&mut reader).ok()?;
        match wt {
            WireType::Varint => {
                let v = reader.read_varint().ok()?;
                lines.push(format!("field {field} (varint): {v}"));
            }
            WireType::Fixed64 => {
                let bytes = reader.read_bytes(8).ok()?;
                lines.push(format!("field {field} (fixed64): {:02x?}", bytes));
            }
            WireType::LengthDelimited => {
                let len = reader.read_varint().ok()? as usize;
                let payload = reader.read_bytes(len).ok()?;
                // Эвристика: если все байты печатные ASCII → строка.
                if payload.iter().all(|&b| (0x20..=0x7e).contains(&b)) && !payload.is_empty() {
                    lines.push(format!(
                        "field {field} (string): \"{}\"",
                        String::from_utf8_lossy(payload)
                    ));
                } else {
                    lines.push(format!(
                        "field {field} (bytes[{len}]): {:02x?}",
                        &payload[..payload.len().min(16)]
                    ));
                }
            }
            WireType::Fixed32 => {
                let bytes = reader.read_bytes(4).ok()?;
                lines.push(format!("field {field} (fixed32): {:02x?}", bytes));
            }
        }
    }
    Some(lines)
}

fn walk(data: &[u8], indent: usize, out: &mut Vec<String>) -> Result<(), TransformError> {
    let pad = "  ".repeat(indent);
    let mut reader = Reader::new(data);

    while reader.pos < data.len() {
        let (field_number, wire_type) =
            read_tag(&mut reader).map_err(|e| TransformError::InvalidInput {
                reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
            })?;

        match wire_type {
            WireType::Varint => {
                let value = reader
                    .read_varint()
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
                    })?;
                out.push(format!("{pad}field {field_number} ({wire_type}): {value}"));
            }
            WireType::Fixed64 => {
                let bytes = reader
                    .read_bytes(8)
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
                    })?;
                out.push(format!(
                    "{pad}field {field_number} ({wire_type}): {:02x?}",
                    bytes
                ));
            }
            WireType::Fixed32 => {
                let bytes = reader
                    .read_bytes(4)
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
                    })?;
                out.push(format!(
                    "{pad}field {field_number} ({wire_type}): {:02x?}",
                    bytes
                ));
            }
            WireType::LengthDelimited => {
                let len = reader
                    .read_varint()
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
                    })? as usize;
                let payload = reader
                    .read_bytes(len)
                    .map_err(|e| TransformError::InvalidInput {
                        reason: format!("protobuf walk error at byte {}: {e}", reader.pos),
                    })?;

                // Эвристика: печатные ASCII → строка; иначе пробуем nested.
                if !payload.is_empty() && payload.iter().all(|&b| (0x20..=0x7e).contains(&b)) {
                    out.push(format!(
                        "{pad}field {field_number} (string[{len}]): \"{}\"",
                        String::from_utf8_lossy(payload)
                    ));
                } else if let Some(nested) = try_parse_nested(payload) {
                    out.push(format!("{pad}field {field_number} (message[{len}]):"));
                    for line in nested {
                        out.push(format!("{pad}  {line}"));
                    }
                } else {
                    let preview_len = payload.len().min(16);
                    out.push(format!(
                        "{pad}field {field_number} (bytes[{len}]): {:02x?}",
                        &payload[..preview_len]
                    ));
                }
            }
        }
    }
    Ok(())
}

pub struct ProtobufDecodeRaw;

impl Transform for ProtobufDecodeRaw {
    fn id(&self) -> &'static str {
        "encoding.protobuf_decode_raw"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Protobuf Decode Raw"
    }
    fn category(&self) -> &'static str {
        "Encoding"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut lines = Vec::new();
        walk(input.as_ref(), 0, &mut lines)?;
        if lines.is_empty() {
            return Err(TransformError::InvalidInput {
                reason: "input is empty or contains no valid protobuf fields".into(),
            });
        }
        Ok(Cow::Owned(lines.join("\n").into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&ProtobufDecodeRaw) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    /// Кодирует varint из u64 (LSB-first, 7 бит за байт).
    fn encode_varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out
    }

    /// Собирает поле tag+varint-value.
    fn field_varint(field: u32, value: u64) -> Vec<u8> {
        let mut tag = encode_varint((field << 3) as u64);
        tag.extend(encode_varint(value));
        tag
    }

    /// Собирает поле tag+len+data.
    fn field_ld(field: u32, data: &[u8]) -> Vec<u8> {
        let mut tag = encode_varint(((field << 3) | 2) as u64);
        tag.extend(encode_varint(data.len() as u64));
        tag.extend_from_slice(data);
        tag
    }

    #[test]
    fn decode_simple_message() {
        // message { string name = 1; int32 id = 2; }
        // Сериализация: field 1 (string) "Alice", field 2 (varint) 42
        let mut input = field_ld(1, b"Alice");
        input.extend(field_varint(2, 42));

        let ctx = NullExecutionContext;
        let out = ProtobufDecodeRaw
            .apply(Cow::Borrowed(&input), &json!({}), &ctx)
            .unwrap();
        let text = String::from_utf8(out.into_owned()).unwrap();

        assert!(text.contains("field 1 (string[5]): \"Alice\""), "{text}");
        assert!(text.contains("field 2 (varint): 42"), "{text}");
    }

    #[test]
    fn decode_nested_message() {
        // inner { int32 x = 1; } → сериализуем как length-delimited поле 3.
        let inner = field_varint(1, 99);
        let outer = field_ld(3, &inner);

        let ctx = NullExecutionContext;
        let out = ProtobufDecodeRaw
            .apply(Cow::Borrowed(&outer), &json!({}), &ctx)
            .unwrap();
        let text = String::from_utf8(out.into_owned()).unwrap();

        assert!(text.contains("field 3 (message[2])"), "{text}");
        assert!(text.contains("field 1 (varint): 99"), "{text}");
    }

    #[test]
    fn decode_fixed32_and_fixed64() {
        let ctx = NullExecutionContext;
        // fixed32: tag=(1<<3)|5=13, затем 4 байта
        // fixed64: tag=(2<<3)|1=17, затем 8 байт
        let mut input = encode_varint(13);
        input.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        input.extend(encode_varint(17));
        input.extend_from_slice(&[0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c]);

        let out = ProtobufDecodeRaw
            .apply(Cow::Borrowed(&input), &json!({}), &ctx)
            .unwrap();
        let text = String::from_utf8(out.into_owned()).unwrap();
        assert!(text.contains("field 1 (fixed32)"), "{text}");
        assert!(text.contains("field 2 (fixed64)"), "{text}");
    }

    #[test]
    fn truncated_input_rejected() {
        let ctx = NullExecutionContext;
        // Обрезанное length-delimited поле: len=100 но данных нет.
        let mut input = encode_varint((1 << 3) | 2);
        input.extend(encode_varint(100));
        input.extend_from_slice(b"only few");

        let err = ProtobufDecodeRaw
            .apply(Cow::Borrowed(&input), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn empty_input_rejected() {
        let ctx = NullExecutionContext;
        let err = ProtobufDecodeRaw
            .apply(Cow::Borrowed(b""), &json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
