use base64::{engine::general_purpose, Engine as _};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct AutoDecode;

impl Transform for AutoDecode {
    fn id(&self) -> &'static str {
        "encoding.auto_decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Auto Decode (Magic Wand)"
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
        let mut current = input.as_ref().to_vec();
        let mut chain: Vec<String> = Vec::new();
        let mut tried: Vec<String> = Vec::new();

        // Chained heuristic up to 5 depths (prevent infinite loop on e.g. "AA" -> hex -> "AA")
        for _ in 0..5 {
            if let Some((name, decoded)) = try_decode_once(&current, &mut tried) {
                // Avoid infinite loop: if decoded == current, break
                if decoded == current {
                    break;
                }
                // Heuristic: decoded should be mostly printable (avoid false positives like "HelloWorld" as base32)
                let printable = decoded
                    .iter()
                    .filter(|b| {
                        (0x20..=0x7e).contains(*b) || **b == b'\n' || **b == b'\r' || **b == b'\t'
                    })
                    .count() as f64
                    / decoded.len().max(1) as f64;
                if printable > 0.7 {
                    chain.push(name);
                    current = decoded;
                    continue;
                }
            }
            break;
        }

        if chain.is_empty() {
            return Err(TransformError::InvalidInput {
                reason: format!(
                    "Magic Wand could not auto-detect encoding (tried: {})",
                    tried.join(", ")
                ),
            });
        }

        let mut out = format!("// detected: {}\n", chain.join(" -> ")).into_bytes();
        out.extend_from_slice(&current);
        Ok(Cow::Owned(out))
    }
}

fn try_decode_once(data: &[u8], tried: &mut Vec<String>) -> Option<(String, Vec<u8>)> {
    // Hex (ignore whitespace) — most specific, roundtrip
    let cleaned: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|&b| b as char)
        .collect();
    if cleaned.len().is_multiple_of(2)
        && !cleaned.is_empty()
        && cleaned.chars().all(|c| c.is_ascii_hexdigit())
    {
        if let Ok(decoded) = hex::decode(&cleaned) {
            if !decoded.is_empty() && hex::encode(&decoded).eq_ignore_ascii_case(&cleaned) {
                return Some(("hex".to_string(), decoded));
            }
        }
    }
    tried.push("hex".to_string());

    // Base64 standard — roundtrip (after hex, before base32 to avoid base32 false positive for base64 strings that contain only base32 alphabet)
    // For "JBSWY3DP" (base32), base64 would also decode but we want base32, so we try base32 before base64.
    // However base64 strings often contain '=' padding and lower case, which base32 would reject (due to 8,9,0,1).
    // So order: hex -> base32 -> base58 -> base64 -> base85 -> url
    let b64_clean: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    // Base32 will be tried next, but we keep base64 here for now and will reorder after base32/base58
    // To avoid duplication, we will try base32 and base58 first, then base64.

    // Base32 (RFC 4648, case-insensitive, ignore whitespace and padding) — roundtrip + length check
    let b32_clean: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace() && **b != b'=')
        .map(|&b| b as char)
        .collect();
    if b32_clean.len() >= 8
        && b32_clean
            .chars()
            .all(|c| matches!(c, 'A'..='Z' | 'a'..='z' | '2'..='7'))
        && b32_clean.len() % 8 != 1
        && !matches!(b32_clean.len() % 8, 1 | 3 | 6)
    {
        if let Ok(decoded) = base32_decode(&b32_clean) {
            if !decoded.is_empty() && decoded != data {
                // Roundtrip: encode must give back the same (case-insensitive, no padding)
                let reenc = base32_encode(&decoded);
                if reenc.eq_ignore_ascii_case(&b32_clean) {
                    return Some(("base32".to_string(), decoded));
                }
            }
        }
    }
    tried.push("base32".to_string());

    // Base58 (Bitcoin alphabet) — roundtrip check
    let b58_clean: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|&b| b as char)
        .collect();
    if !b58_clean.is_empty()
        && b58_clean
            .chars()
            .all(|c| "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(c))
    {
        if let Ok(decoded) = base58_decode_auto(&b58_clean) {
            if !decoded.is_empty() && decoded != data {
                let reenc = base58_encode_auto(&decoded);
                if reenc == b58_clean {
                    return Some(("base58".to_string(), decoded));
                }
            }
        }
    }
    tried.push("base58".to_string());

    // Base64 standard — roundtrip
    if let Ok(decoded) = general_purpose::STANDARD.decode(&b64_clean) {
        if !decoded.is_empty() && decoded != data {
            let reenc = general_purpose::STANDARD.encode(&decoded);
            if reenc.as_bytes() == b64_clean.as_slice() {
                return Some(("base64".to_string(), decoded));
            }
        }
    }
    tried.push("base64".to_string());
    // Base64 url_safe — roundtrip
    if let Ok(decoded) = general_purpose::URL_SAFE.decode(&b64_clean) {
        if !decoded.is_empty() && decoded != data {
            let reenc = general_purpose::URL_SAFE.encode(&decoded);
            if reenc.as_bytes() == b64_clean.as_slice() {
                return Some(("base64_url".to_string(), decoded));
            }
        }
    }
    tried.push("base64_url".to_string());

    // Base85 (Ascii85, 4->5, z for zeros) — roundtrip check
    let s = String::from_utf8_lossy(data);
    if !s.trim().is_empty()
        && s.chars()
            .all(|c| c == 'z' || ('!'..='u').contains(&c) || c.is_ascii_whitespace())
    {
        if let Ok(decoded) = base85_decode(&s) {
            if !decoded.is_empty() && decoded != data {
                let reenc = base85_encode(&decoded);
                // Normalize z: both "z" and "!!!!!" decode to 4 zeros, but encode gives "z"
                let norm_original = s
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .collect::<String>()
                    .replace('z', "!!!!!");
                let norm_reenc = reenc.replace('z', "!!!!!");
                if norm_reenc == norm_original
                    || reenc
                        == s.chars()
                            .filter(|c| !c.is_ascii_whitespace())
                            .collect::<String>()
                {
                    return Some(("base85".to_string(), decoded));
                }
            }
        }
    }
    tried.push("base85".to_string());

    // URL percent-encoding (RFC 3986, + -> space) — roundtrip check
    if data.contains(&b'%') {
        if let Ok(decoded) = url_decode(data) {
            if decoded != data {
                let reenc = url_encode(&decoded);
                // Compare after normalizing: url_encode should give back original (case-insensitive % hex)
                if reenc.eq_ignore_ascii_case(&String::from_utf8_lossy(data)) {
                    return Some(("url".to_string(), decoded));
                }
            }
        }
    }
    tried.push("url".to_string());

    None
}

// Minimal base32 decode (RFC 4648, no external crate for auto)
fn base32_decode(s: &str) -> Result<Vec<u8>, String> {
    let upper = s.to_ascii_uppercase();
    let mut bits: u32 = 0;
    let mut bits_left = 0;
    let mut out = Vec::new();
    for c in upper.chars() {
        if c == '=' {
            break;
        }
        let val = match c {
            'A'..='Z' => (c as u8 - b'A') as u32,
            '2'..='7' => (c as u8 - b'2' + 26) as u32,
            _ => return Err(format!("invalid base32 char {c}")),
        };
        bits = (bits << 5) | val;
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push((bits >> bits_left) as u8);
            bits &= (1 << bits_left) - 1;
        }
    }
    Ok(out)
}

fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut bits: u32 = 0;
    let mut bits_left = 0;
    for &b in data {
        bits = (bits << 8) | b as u32;
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            out.push(ALPHABET[((bits >> bits_left) & 0x1f) as usize] as char);
        }
    }
    if bits_left > 0 {
        out.push(ALPHABET[((bits << (5 - bits_left)) & 0x1f) as usize] as char);
    }
    out
}

// Minimal base85 decode (Ascii85, z -> 4 zeros, 4->5)
fn base85_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut filtered = String::new();
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == 'z' {
            filtered.push_str("!!!!!");
        } else {
            filtered.push(c);
        }
    }
    if filtered.is_empty() {
        return Ok(Vec::new());
    }
    for c in filtered.chars() {
        if !('!'..='u').contains(&c) {
            return Err(format!("invalid base85 char {c}"));
        }
    }
    let mut padded = filtered.clone();
    let rem = padded.len() % 5;
    if rem != 0 {
        for _ in 0..(5 - rem) {
            padded.push('u');
        }
    }
    let mut out = Vec::new();
    for chunk in padded.as_bytes().chunks(5) {
        let mut n: u32 = 0;
        for &c in chunk {
            n = n * 85 + (c - 33) as u32;
        }
        out.push((n >> 24) as u8);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
    }
    let groups = filtered.len().div_ceil(5);
    let total_bytes = groups * 4;
    let pad_chars = (5 - (filtered.len() % 5)) % 5;
    let truncate = match pad_chars {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 3,
        _ => 0,
    };
    if rem != 0 && rem != 1 {
        out.truncate(total_bytes - truncate);
    } else if rem == 1 {
        return Err("invalid base85 length".into());
    }
    Ok(out)
}

// Minimal base58 decode (Bitcoin alphabet, same as base58.rs)
fn base58_decode_auto(s: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut table = [-1i8; 128];
    for (i, &c) in ALPHABET.iter().enumerate() {
        table[c as usize] = i as i8;
    }
    let filtered: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if filtered.is_empty() {
        return Ok(vec![]);
    }
    let zeros = filtered.iter().take_while(|&&b| b == ALPHABET[0]).count();
    let mut bytes: Vec<u8> = Vec::new();
    for &c in &filtered[zeros..] {
        if (c as usize) >= 128 || table[c as usize] < 0 {
            return Err(format!("invalid base58 char '{}'", c as char));
        }
        let mut carry = table[c as usize] as u32;
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
    let mut out = vec![0u8; zeros];
    for b in bytes.iter().rev() {
        out.push(*b);
    }
    Ok(out)
}

fn base58_encode_auto(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if data.is_empty() {
        return String::new();
    }
    let zeros = data.iter().take_while(|&&b| b == 0).count();
    let mut digits: Vec<u8> = Vec::new();
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
    let mut out = String::new();
    for _ in 0..zeros {
        out.push(ALPHABET[0] as char);
    }
    for d in digits.iter().rev() {
        out.push(ALPHABET[*d as usize] as char);
    }
    out
}

fn base85_encode(data: &[u8]) -> String {
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
            for &c in &chars[..remaining + 1] {
                out.push(c as char);
            }
            break;
        }
    }
    out
}

fn url_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for &b in data {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push_str("%20");
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// Minimal URL decode (percent + -> space)
fn url_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'%' => {
                if i + 2 >= data.len() {
                    return Err("truncated percent".into());
                }
                let hi = (data[i + 1] as char).to_digit(16).ok_or("invalid hex")?;
                let lo = (data[i + 2] as char).to_digit(16).ok_or("invalid hex")?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Ok(out)
}

inventory::submit! { crate::TransformEntry(&AutoDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn detects_base64() {
        let ctx = NullExecutionContext;
        let b64 = general_purpose::STANDARD.encode(b"Hello World");
        let out = AutoDecode
            .apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.ends_with(b"Hello World"));
    }

    #[test]
    fn detects_hex() {
        let ctx = NullExecutionContext;
        let out = AutoDecode
            .apply(Cow::Borrowed(b"48656c6c6f"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.ends_with(b"Hello"));
    }

    #[test]
    fn detects_base32() {
        let ctx = NullExecutionContext;
        // "Hello" -> base32 "JBSWY3DP"
        let out = AutoDecode
            .apply(Cow::Borrowed(b"JBSWY3DP"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.ends_with(b"Hello"));
    }

    #[test]
    fn detects_base58() {
        let ctx = NullExecutionContext;
        // "Hello World" -> base58 "JxF12TrwUP45BMd" (Bitcoin alphabet, from base58::tests)
        let out = AutoDecode
            .apply(
                Cow::Borrowed(b"JxF12TrwUP45BMd"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert!(out.ends_with(b"Hello World"));
    }

    #[test]
    fn detects_url() {
        let ctx = NullExecutionContext;
        let out = AutoDecode
            .apply(
                Cow::Borrowed(b"Hello%20World%21"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert!(out.ends_with(b"Hello World!"));
    }

    #[test]
    fn detects_chained_base64_hex() {
        let ctx = NullExecutionContext;
        // "Hi" -> hex "4869" -> base64 "NDg2OQ=="
        let hexed = hex::encode(b"Hi");
        let b64 = general_purpose::STANDARD.encode(hexed.as_bytes());
        let out = AutoDecode
            .apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        // Should detect chain base64 -> hex
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.windows(3).any(|w| w == b"hex"));
        assert!(out.ends_with(b"Hi"));
    }

    #[test]
    fn rejects_unknown() {
        let ctx = NullExecutionContext;
        let err = AutoDecode
            .apply(Cow::Borrowed(b"\x00\xFF\xFE"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
