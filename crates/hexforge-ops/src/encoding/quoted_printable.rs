use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

fn qp_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 3);
    let mut line_len = 0usize;
    for &b in data {
        let needs_encode = b == b'=' || !(33..=126).contains(&b);
        let chunk_len = if needs_encode { 3 } else { 1 };
        // Soft line break at 76 chars (RFC 2045)
        if line_len + chunk_len > 76 {
            out.extend_from_slice(b"=\r\n");
            line_len = 0;
        }
        if needs_encode {
            out.push(b'=');
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0F));
            line_len += 3;
        } else {
            out.push(b);
            line_len += 1;
        }
    }
    out
}

fn hex_digit(n: u8) -> u8 {
    match n {
        0..=9 => b'0' + n,
        _ => b'A' + n - 10,
    }
}

fn qp_decode(data: &[u8]) -> Result<Vec<u8>, TransformError> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0usize;
    while i < data.len() {
        if data[i] == b'=' {
            // Soft line break "=\r\n" or "=\n"
            if i + 1 < data.len() && data[i + 1] == b'\r' {
                if i + 2 < data.len() && data[i + 2] == b'\n' {
                    i += 3;
                    continue;
                }
            } else if i + 1 < data.len() && data[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            // Expect =XX hex
            if i + 2 >= data.len() {
                return Err(TransformError::InvalidInput {
                    reason: format!("truncated quoted-printable escape at byte {i}"),
                });
            }
            let hi =
                (data[i + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| TransformError::InvalidInput {
                        reason: format!(
                            "invalid quoted-printable escape at byte {i}: '{}' is not hex",
                            data[i + 1] as char
                        ),
                    })? as u8;
            let lo =
                (data[i + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| TransformError::InvalidInput {
                        reason: format!(
                            "invalid quoted-printable escape at byte {i}: '{}' is not hex",
                            data[i + 2] as char
                        ),
                    })? as u8;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    Ok(out)
}

pub struct QuotedPrintableEncode;

impl Transform for QuotedPrintableEncode {
    fn id(&self) -> &'static str {
        "encoding.quoted_printable.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Quoted-Printable Encode"
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
        Ok(Cow::Owned(qp_encode(input.as_ref())))
    }
}

pub struct QuotedPrintableDecode;

impl Transform for QuotedPrintableDecode {
    fn id(&self) -> &'static str {
        "encoding.quoted_printable.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Quoted-Printable Decode"
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
        Ok(Cow::Owned(qp_decode(input.as_ref())?))
    }
}

inventory::submit! { crate::TransformEntry(&QuotedPrintableEncode) }
inventory::submit! { crate::TransformEntry(&QuotedPrintableDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn roundtrip() {
        let ctx = NullExecutionContext;
        for data in [b"Hello World" as &[u8], b"=", b"\x00\xFF", b"a=b c"] {
            let enc = QuotedPrintableEncode
                .apply(Cow::Borrowed(data), &serde_json::json!({}), &ctx)
                .unwrap();
            let dec = QuotedPrintableDecode
                .apply(enc, &serde_json::json!({}), &ctx)
                .unwrap();
            assert_eq!(dec.as_ref(), data);
        }
    }

    #[test]
    fn encode_equals_and_nonprintable() {
        let ctx = NullExecutionContext;
        let out = QuotedPrintableEncode
            .apply(Cow::Borrowed(b"a=b"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"a=3Db");
    }

    #[test]
    fn soft_break_ignored_on_decode() {
        let ctx = NullExecutionContext;
        let dec = QuotedPrintableDecode
            .apply(
                Cow::Borrowed(b"Hello=\r\nWorld"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert_eq!(dec.as_ref(), b"HelloWorld");
    }

    #[test]
    fn decode_rejects_invalid_escape() {
        let ctx = NullExecutionContext;
        let err = QuotedPrintableDecode
            .apply(Cow::Borrowed(b"=ZZ"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
        let err2 = QuotedPrintableDecode
            .apply(Cow::Borrowed(b"=A"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err2, TransformError::InvalidInput { .. }));
    }
}
