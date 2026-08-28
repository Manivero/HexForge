use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn b58_value(c: u8) -> Option<u8> {
    const TABLE: [i8; 128] = {
        let mut t = [-1i8; 128];
        let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        let mut i = 0;
        while i < alphabet.len() {
            t[alphabet[i] as usize] = i as i8;
            i += 1;
        }
        t
    };
    if (c as usize) < 128 {
        let v = TABLE[c as usize];
        if v >= 0 {
            Some(v as u8)
        } else {
            None
        }
    } else {
        None
    }
}

fn encode_base58(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![];
    }
    // Count leading zeros.
    let zeros = data.iter().take_while(|&&b| b == 0).count();
    // Convert base256 to base58.
    let mut digits: Vec<u8> = Vec::new(); // little-endian base58 digits
    for &b in &data[zeros..] {
        let mut carry = b as u32;
        for d in digits.iter_mut() {
            let cur = *d as u32 * 256 + carry;
            *d = (cur % 58) as u8;
            carry = cur / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out = Vec::with_capacity(zeros + digits.len());
    for _ in 0..zeros {
        out.push(ALPHABET[0]);
    }
    for d in digits.iter().rev() {
        out.push(ALPHABET[*d as usize]);
    }
    out
}

fn decode_base58(s: &[u8]) -> Result<Vec<u8>, TransformError> {
    // Strip whitespace
    let filtered: Vec<u8> = s
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if filtered.is_empty() {
        return Ok(vec![]);
    }
    let zeros = filtered.iter().take_while(|&&b| b == ALPHABET[0]).count();
    let mut bytes: Vec<u8> = Vec::new(); // little-endian base256
    for &c in &filtered[zeros..] {
        let val = b58_value(c).ok_or_else(|| TransformError::InvalidInput {
            reason: format!("invalid base58 character '{}'", c as char),
        })? as u32;
        let mut carry = val;
        for b in bytes.iter_mut() {
            let cur = *b as u32 * 58 + carry;
            *b = (cur % 256) as u8;
            carry = cur / 256;
        }
        while carry > 0 {
            bytes.push((carry % 256) as u8);
            carry /= 256;
        }
    }
    // bytes is little-endian, need to reverse and prepend zeros
    let mut out = vec![0u8; zeros];
    for b in bytes.iter().rev() {
        out.push(*b);
    }
    Ok(out)
}

pub struct Base58Encode;

impl Transform for Base58Encode {
    fn id(&self) -> &'static str {
        "encoding.base58.encode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base58 Encode"
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
        Ok(Cow::Owned(encode_base58(input.as_ref())))
    }
}

pub struct Base58Decode;

impl Transform for Base58Decode {
    fn id(&self) -> &'static str {
        "encoding.base58.decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Base58 Decode"
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
        Ok(Cow::Owned(decode_base58(input.as_ref())?))
    }
}

inventory::submit! { crate::TransformEntry(&Base58Encode) }
inventory::submit! { crate::TransformEntry(&Base58Decode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn known_vectors() {
        let ctx = NullExecutionContext;
        // Bitcoin base58 vector: Hello World
        let out = Base58Encode
            .apply(Cow::Borrowed(b"Hello World"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"JxF12TrwUP45BMd");
        let dec = Base58Decode
            .apply(out, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"Hello World");

        // Empty
        let out = Base58Encode
            .apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"");
        let dec = Base58Decode
            .apply(out, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"");

        // Leading zeros → '1' prefix and roundtrip
        let plain = b"\x00\x00Hello";
        let enc = Base58Encode
            .apply(Cow::Borrowed(plain), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(
            enc.starts_with(b"11"),
            "leading zeros must become '1's, got {}",
            String::from_utf8_lossy(&enc)
        );
        let dec = Base58Decode
            .apply(enc, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), plain.as_slice());
    }

    #[test]
    fn roundtrip_random() {
        let ctx = NullExecutionContext;
        for len in 0..32 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 13) as u8).collect();
            let enc = Base58Encode
                .apply(Cow::Borrowed(&data), &serde_json::json!({}), &ctx)
                .unwrap();
            let dec = Base58Decode
                .apply(enc, &serde_json::json!({}), &ctx)
                .unwrap();
            assert_eq!(dec.as_ref(), data.as_slice(), "len {len}");
        }
    }

    #[test]
    fn rejects_invalid_char() {
        let ctx = NullExecutionContext;
        let err = Base58Decode
            .apply(Cow::Borrowed(b"0OIl"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn whitespace_ignored() {
        let ctx = NullExecutionContext;
        let enc = Base58Encode
            .apply(Cow::Borrowed(b"Hello"), &serde_json::json!({}), &ctx)
            .unwrap();
        // Insert whitespace and decode
        let spaced = [enc.as_ref(), b"  \n "].concat();
        let dec = Base58Decode
            .apply(Cow::Borrowed(&spaced), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(dec.as_ref(), b"Hello");
    }
}
