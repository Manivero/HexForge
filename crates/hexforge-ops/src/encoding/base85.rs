use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

/// Ascii85 (Base85) — 4 bytes -> 5 chars in range '!'..'u' (33..117), 'z' for 4 zeros.
/// No external crate: pure stdlib per project rule to avoid extra deps.
fn encode_base85(data: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let remaining = data.len() - i;
        if remaining >= 4 {
            let chunk = &data[i..i + 4];
            if chunk == [0, 0, 0, 0] {
                out.push('z');
            } else {
                let mut n = ((chunk[0] as u32) << 24)
                    | ((chunk[1] as u32) << 16)
                    | ((chunk[2] as u32) << 8)
                    | (chunk[3] as u32);
                let mut chars = [0u8; 5];
                for j in (0..5).rev() {
                    chars[j] = (n % 85) as u8 + 33;
                    n /= 85;
                }
                for &c in &chars {
                    out.push(c as char);
                }
            }
            i += 4;
        } else {
            // Final partial group: pad with zeros, encode, then truncate
            let mut padded = [0u8; 4];
            padded[..remaining].copy_from_slice(&data[i..]);
            let mut n = ((padded[0] as u32) << 24)
                | ((padded[1] as u32) << 16)
                | ((padded[2] as u32) << 8)
                | (padded[3] as u32);
            let mut chars = [0u8; 5];
            for j in (0..5).rev() {
                chars[j] = (n % 85) as u8 + 33;
                n /= 85;
            }
            // Output only remaining+1 chars
            for &c in &chars[..remaining + 1] {
                out.push(c as char);
            }
            break;
        }
    }
    out
}

fn decode_base85(s: &str) -> Result<Vec<u8>, String> {
    // Filter whitespace, handle 'z'
    let mut filtered = String::new();
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == 'z' {
            // 'z' must stand alone as group of 4 zeros; expand to 5 '!' (which decodes to zeros) then handle via normal path
            // But we expand directly to bytes
            // For simplicity, we treat 'z' as 4 zero bytes directly in output,
            // but to keep group handling uniform, we expand to "!!!!!" which decodes to 0
            // Instead, we handle 'z' by directly pushing 4 zeros and continuing
            // However we need to ensure it's not inside a 5-char group
            // So we flush any pending partial? In Ascii85, 'z' only appears where a 5-char group would be
            // We handle by checking filtered length %5 ==0 before 'z' is added
            // Simplify: if we encounter 'z', it replaces a full 5-char group
            // We can just directly output 4 zeros if we're at group boundary
            // For now, we require 'z' to be at group boundary (filtered len %5 ==0 before push)
            // But we handle by expanding to 4 zeros directly via special handling outside this loop?
            // Instead, we will expand 'z' to "!!!!!" in filtered string, then normal decode will produce 4 zeros
            filtered.push_str("!!!!!");
        } else {
            filtered.push(c);
        }
    }

    if filtered.is_empty() {
        return Ok(Vec::new());
    }

    // Validate chars in '!'..'u'
    for c in filtered.chars() {
        if c == '=' {
            // Padding not used in Ascii85; ignore?
            continue;
        }
        if !(33u8..=117u8).contains(&(c as u8)) {
            return Err(format!("invalid base85 character '{}'", c));
        }
    }

    // Pad final group with 'u' (84) to 5 if needed
    let mut padded = filtered.clone();
    let rem = padded.len() % 5;
    if rem != 0 {
        let needed = 5 - rem;
        for _ in 0..needed {
            padded.push('u');
        }
    }

    let mut out = Vec::new();
    for chunk in padded.as_bytes().chunks(5) {
        if chunk.len() != 5 {
            return Err("invalid chunk length".into());
        }
        let mut n: u32 = 0;
        for &c in chunk {
            if !(33..=117).contains(&c) {
                return Err(format!("invalid char {}", c as char));
            }
            n = n * 85 + (c - 33) as u32;
        }
        // n is 32-bit value, split to 4 bytes
        out.push((n >> 24) as u8);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }

    // Truncate output based on original filtered length (without padding we added)
    // Original groups: ceil(filtered_len *4/5) bytes, but we padded filtered to multiple of 5, and output 4*groups
    // Real output length = floor(filtered_len *4 /5) ??? For Ascii85, 5 chars ->4 bytes, so bytes = groups*4 - pad_bytes
    // Pad_bytes = (5 - rem) %5 truncated to bytes: if rem==0 =>0, rem==2 =>3 bytes pad? Actually 2 chars ->1 byte, we padded 3 'u's, we produced 4 bytes, but should only keep 1 byte
    // So truncate: if rem==0 => keep all, rem==2 => keep 1 byte of last group, rem==3 =>2 bytes, rem==4 =>3 bytes
    let groups = filtered.len().div_ceil(5);
    let total_bytes = groups * 4;
    let pad_chars = (5 - (filtered.len() % 5)) % 5;
    let truncate = match pad_chars {
        0 => 0,
        1 => 1, // 4 chars ->3 bytes, but we padded 1 'u', we produced 4 bytes, need 3
        2 => 2, // 3 chars ->2 bytes, padded 2, produced 4, need 2
        3 => 3, // 2 chars ->1 byte, padded 3, need 1
        4 => 3, // 1 char invalid already rejected, but treat as 3
        _ => 0,
    };
    // Actually for rem=1 we already error, but handle
    if rem != 0 && rem != 1 {
        out.truncate(total_bytes - truncate);
    } else if rem == 0 {
        // no truncate
    } else {
        // rem==1 should have been error earlier, but if we padded, we still need to handle
        return Err("invalid base85 length (1 char leftover)".into());
    }

    // Special handling for 'z' expansion: we expanded 'z' to "!!!!!" which decodes to 4 zeros correctly, no extra handling needed
    Ok(out)
}

pub struct Base85Encode;

impl Transform for Base85Encode {
    fn id(&self) -> &'static str {
        "encoding.base85.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base85 Encode"
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
        Ok(Cow::Owned(encode_base85(input.as_ref()).into_bytes()))
    }
}

pub struct Base85Decode;

impl Transform for Base85Decode {
    fn id(&self) -> &'static str {
        "encoding.base85.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base85 Decode"
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
        let s = String::from_utf8_lossy(input.as_ref());
        let decoded = decode_base85(&s).map_err(|e| TransformError::InvalidInput {
            reason: format!("not valid base85: {e}"),
        })?;
        Ok(Cow::Owned(decoded))
    }
}

inventory::submit! { crate::TransformEntry(&Base85Encode) }
inventory::submit! { crate::TransformEntry(&Base85Decode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn roundtrip() {
        let ctx = NullExecutionContext;
        let data = b"Hello Base85 world! 12345";
        let enc = Base85Encode
            .apply(Cow::Borrowed(data), &serde_json::json!({}), &ctx)
            .unwrap();
        let dec = Base85Decode
            .apply(enc, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), data);
    }

    #[test]
    fn zeros_encode_as_z() {
        let ctx = NullExecutionContext;
        let enc = Base85Encode
            .apply(Cow::Borrowed(&[0, 0, 0, 0]), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(enc.as_ref(), b"z");
        let dec = Base85Decode
            .apply(enc, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), &[0, 0, 0, 0]);
    }

    #[test]
    fn partial_groups() {
        let ctx = NullExecutionContext;
        for len in 1..7 {
            let data = vec![0xAB; len];
            let enc = Base85Encode
                .apply(Cow::Borrowed(&data), &serde_json::json!({}), &ctx)
                .unwrap();
            let dec = Base85Decode
                .apply(enc, &serde_json::json!({}), &ctx)
                .unwrap();
            assert_eq!(dec.as_ref(), data.as_slice(), "len {len}");
        }
    }

    #[test]
    fn known_vector() {
        // "Man" -> Ascii85 example: "Man " -> "9jqo^" per spec
        let ctx = NullExecutionContext;
        let enc = Base85Encode
            .apply(Cow::Borrowed(b"Man "), &serde_json::json!({}), &ctx)
            .unwrap();
        // "Man " in Ascii85 is "9jqo^" (from Adobe example)
        assert_eq!(enc.as_ref(), b"9jqo^");
        let dec = Base85Decode
            .apply(Cow::Borrowed(b"9jqo^"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"Man ");
    }

    #[test]
    fn rejects_invalid() {
        let ctx = NullExecutionContext;
        let err = Base85Decode
            .apply(Cow::Borrowed(b"\xFF\xFE"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
