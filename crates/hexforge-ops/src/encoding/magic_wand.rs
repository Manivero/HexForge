//! `encoding.magic_wand` — детерминированный «Magic Wand» для автоматического
//! определения цепочки decode/decompress преобразований (PRD FR-3.9).
//!
//! Алгоритм: DFS с мемоизацией (visited-set) и глубинной ограничением `MAX_DEPTH`.
//! На каждом шаге перебираются все доступные декодеры. Промежуточные результаты
//! НЕ оцениваются — вместо этого запоминается лучший ФИНАЛЬНЫЙ результат
//! (по score). Это позволяет находить цепочки вида base64 → gzip → "hello",
//! где промежуточный gzip-bytes непечатен, но финальный результат хорош.
//!
//! Ограничения:
//! - Только детерминированные decode/decompress операции.
//! - XOR не поддерживается (требует внешний key).
//! - Защита от циклов через visited-set.
//! - `MAX_DEPTH = 5` предотвращает explosion.

use base64::{engine::general_purpose, Engine as _};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Read;

/// Максимальная глубина цепочки decode-операций.
const MAX_DEPTH: usize = 5;

/// Минимальный printable ratio для финального результата.
const MIN_PRINTABLE_RATIO: f64 = 0.7;

/// Операция `Magic Wand` — эвристический авто-декодер.
pub struct MagicWand;

impl Transform for MagicWand {
    fn id(&self) -> &'static str {
        "encoding.magic_wand"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Magic Wand"
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
        let original = input.as_ref().to_vec();

        // Check if the original input itself is already a good result.
        let mut best_result: Option<(Vec<u8>, Vec<String>)> = None;
        let mut best_score: u64 = 0;

        if is_acceptable_final(&original) {
            best_score = score_bytes(&original);
            best_result = Some((original.clone(), vec![]));
        }

        let mut visited: HashSet<Vec<u8>> = HashSet::new();
        visited.insert(original.clone());

        dfs(
            &original,
            &[],
            0,
            &mut visited,
            &mut best_result,
            &mut best_score,
        );

        let (final_bytes, chain) = best_result.ok_or_else(|| TransformError::InvalidInput {
            reason: "Magic Wand could not detect any known encoding".into(),
        })?;

        // Результат: цепочка в виде диагностического заголовка + финальные bytes.
        let mut out = format!("// magic_wand: {}\n", chain.join(" -> ")).into_bytes();
        out.extend_from_slice(&final_bytes);
        Ok(Cow::Owned(out))
    }
}

/// DFS поиска лучшей цепочки decode-операций.
fn dfs(
    current: &[u8],
    chain: &[String],
    depth: usize,
    visited: &mut HashSet<Vec<u8>>,
    best_result: &mut Option<(Vec<u8>, Vec<String>)>,
    best_score: &mut u64,
) {
    if depth >= MAX_DEPTH {
        return;
    }

    // Try all decoders.
    for (name, decode_fn) in ALL_DECODERS {
        if let Some(decoded) = decode_fn(current) {
            if decoded.is_empty() || decoded.as_slice() == current {
                continue;
            }
            if visited.contains(&decoded) {
                continue;
            }

            let mut new_chain = chain.to_vec();
            new_chain.push(name.to_string());

            // Check if this is a good final result.
            // Score includes a small bonus for chain depth so deeper chains are preferred.
            let score = score_bytes(&decoded).saturating_add(new_chain.len() as u64);
            if is_acceptable_final(&decoded) && score > *best_score {
                *best_score = score;
                *best_result = Some((decoded.clone(), new_chain.clone()));
            }

            visited.insert(decoded.clone());
            dfs(
                &decoded,
                &new_chain,
                depth + 1,
                visited,
                best_result,
                best_score,
            );
            // Keep in visited to avoid revisiting the same state via different chains.
        }
    }
}

/// Список всех доступных декодеров.
static ALL_DECODERS: &[(&str, fn(&[u8]) -> Option<Vec<u8>>)] = &[
    ("hex", try_hex),
    ("base32", try_base32),
    ("base58", try_base58),
    ("base64", try_base64_standard),
    ("base64_url", try_base64_url),
    ("base85", try_base85),
    ("url", try_url_decode),
    ("gzip", try_gzip_decompress),
    ("zlib", try_zlib_decompress),
    ("bzip2", try_bzip2_decompress),
    ("lzma", try_lzma_decompress),
];

/// Проверить, что результат «приемлем» как финальный — не пустой, printable ratio > threshold.
fn is_acceptable_final(data: &[u8]) -> bool {
    if data.is_empty() || data.len() > 10 * 1024 * 1024 {
        return false;
    }
    let printable = data
        .iter()
        .filter(|b| (0x20..=0x7e).contains(*b) || **b == b'\n' || **b == b'\r' || **b == b'\t')
        .count() as f64
        / data.len().max(1) as f64;
    printable >= MIN_PRINTABLE_RATIO
}

/// Оценить качество результа — чем выше, тем лучше.
fn score_bytes(data: &[u8]) -> u64 {
    if data.is_empty() {
        return 0;
    }
    let mut score: u64 = 0;

    // UTF-8 validity — сильный сигнал.
    if std::str::from_utf8(data).is_ok() {
        score += 100;
    }

    // Printable ratio.
    let printable = data
        .iter()
        .filter(|b| (0x20..=0x7e).contains(*b) || **b == b'\n' || **b == b'\r' || **b == b'\t')
        .count() as f64
        / data.len().max(1) as f64;
    if printable > 0.9 {
        score += 50;
    } else if printable > 0.7 {
        score += 30;
    }

    // Entropy: низкая энтропия (текст) лучше высокой (случайные байты).
    let entropy = compute_entropy(data);
    if entropy < 4.0 {
        score += 20;
    } else if entropy < 5.5 {
        score += 10;
    }

    score
}

/// Посчитать Shannon entropy (bits per byte).
fn compute_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0u64; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let len = data.len() as f64;
    let mut entropy = 0.0_f64;
    for &count in &counts {
        if count == 0 {
            continue;
        }
        let p = count as f64 / len;
        entropy -= p * p.log2();
    }
    entropy
}

// ---------- decoder helpers (roundtrip-based) ----------

/// Hex decode с roundtrip verification.
fn try_hex(data: &[u8]) -> Option<Vec<u8>> {
    let cleaned: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|&b| b as char)
        .collect();
    if cleaned.is_empty() || !cleaned.len().is_multiple_of(2) {
        return None;
    }
    if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let decoded = hex::decode(&cleaned).ok()?;
    if decoded.is_empty() {
        return None;
    }
    // Roundtrip: encode must match (case-insensitive).
    let reenc = hex::encode(&decoded);
    if !reenc.eq_ignore_ascii_case(&cleaned) {
        return None;
    }
    // Avoid no-op: decoded must differ from original.
    if decoded.as_slice() == data {
        return None;
    }
    Some(decoded)
}

/// Base32 decode (RFC 4648) с roundtrip verification.
fn try_base32(data: &[u8]) -> Option<Vec<u8>> {
    let cleaned: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace() && **b != b'=')
        .map(|&b| b as char)
        .collect();
    if cleaned.len() < 8 {
        return None;
    }
    if !cleaned
        .chars()
        .all(|c| matches!(c, 'A'..='Z' | 'a'..='z' | '2'..='7'))
    {
        return None;
    }
    let len_mod = cleaned.len() % 8;
    if len_mod == 1 || len_mod == 3 || len_mod == 6 {
        return None;
    }
    let decoded = base32_decode_upper(&cleaned)?;
    if decoded.is_empty() || decoded.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = base32_encode_upper(&decoded);
    if !reenc.eq_ignore_ascii_case(&cleaned) {
        return None;
    }
    Some(decoded)
}

fn base32_decode_upper(s: &str) -> Option<Vec<u8>> {
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
            _ => return None,
        };
        bits = (bits << 5) | val;
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push((bits >> bits_left) as u8);
            bits &= (1 << bits_left) - 1;
        }
    }
    Some(out)
}

fn base32_encode_upper(data: &[u8]) -> String {
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

/// Base58 decode (Bitcoin alphabet) с roundtrip verification.
fn try_base58(data: &[u8]) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let cleaned: String = data
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|&b| b as char)
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    if !cleaned.chars().all(|c| ALPHABET.contains(&(c as u8))) {
        return None;
    }
    // Leading '1's → leading 0x00 bytes.
    let zeros = cleaned.chars().take_while(|&c| c == '1').count();
    let mut bytes: Vec<u8> = Vec::new();
    for c in cleaned[zeros..].chars() {
        let idx = ALPHABET.iter().position(|&x| x == c as u8)?;
        let mut carry = idx as u32;
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
    out.extend(bytes.iter().rev());
    if out.is_empty() || out.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = base58_encode(&out);
    if reenc != cleaned {
        return None;
    }
    Some(out)
}

fn base58_encode(data: &[u8]) -> String {
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

/// Base64 standard decode с roundtrip verification.
fn try_base64_standard(data: &[u8]) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.len() < 4 || !cleaned.len().is_multiple_of(4) {
        return None;
    }
    // Alphabet check.
    if !cleaned
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/' || *b == b'=')
    {
        return None;
    }
    let decoded = general_purpose::STANDARD.decode(&cleaned).ok()?;
    if decoded.is_empty() || decoded.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = general_purpose::STANDARD.encode(&decoded);
    if reenc.as_bytes() != cleaned.as_slice() {
        return None;
    }
    Some(decoded)
}

/// Base64 URL-safe decode с roundtrip verification.
fn try_base64_url(data: &[u8]) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.len() < 4 || !cleaned.len().is_multiple_of(4) {
        return None;
    }
    // Alphabet check: no '+' or '/', but '-' and '_'.
    if cleaned.iter().any(|b| *b == b'+' || *b == b'/') {
        return None;
    }
    if !cleaned
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' || *b == b'=')
    {
        return None;
    }
    let decoded = general_purpose::URL_SAFE.decode(&cleaned).ok()?;
    if decoded.is_empty() || decoded.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = general_purpose::URL_SAFE.encode(&decoded);
    if reenc.as_bytes() != cleaned.as_slice() {
        return None;
    }
    Some(decoded)
}

/// Base85 (Ascii85) decode с roundtrip verification.
fn try_base85(data: &[u8]) -> Option<Vec<u8>> {
    let s = String::from_utf8_lossy(data);
    let trimmed: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed
        .chars()
        .all(|c| c == 'z' || ('!'..='u').contains(&c))
    {
        return None;
    }
    let decoded = base85_decode(&trimmed)?;
    if decoded.is_empty() || decoded.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = base85_encode(&decoded);
    let norm_original = trimmed.replace('z', "!!!!!");
    let norm_reenc = reenc.replace('z', "!!!!!");
    if norm_reenc != norm_original {
        return None;
    }
    Some(decoded)
}

fn base85_decode(s: &str) -> Option<Vec<u8>> {
    let mut filtered = String::new();
    for c in s.chars() {
        if c == 'z' {
            filtered.push_str("!!!!!");
        } else {
            filtered.push(c);
        }
    }
    if filtered.is_empty() {
        return Some(Vec::new());
    }
    for c in filtered.chars() {
        if !('!'..='u').contains(&c) {
            return None;
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
        let mut n: u64 = 0;
        for &c in chunk {
            n = n * 85 + (c - 33) as u64;
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
        return None;
    }
    Some(out)
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
                let mut n = ((chunk[0] as u64) << 24)
                    | ((chunk[1] as u64) << 16)
                    | ((chunk[2] as u64) << 8)
                    | (chunk[3] as u64);
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
            let mut n = ((padded[0] as u64) << 24)
                | ((padded[1] as u64) << 16)
                | ((padded[2] as u64) << 8)
                | (padded[3] as u64);
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

/// URL percent-decode с roundtrip verification.
fn try_url_decode(data: &[u8]) -> Option<Vec<u8>> {
    if !data.contains(&b'%') {
        return None;
    }
    let decoded = url_decode_internal(data)?;
    if decoded.as_slice() == data {
        return None;
    }
    // Roundtrip.
    let reenc = url_encode_internal(&decoded);
    if !reenc.eq_ignore_ascii_case(&String::from_utf8_lossy(data)) {
        return None;
    }
    Some(decoded)
}

fn url_encode_internal(data: &[u8]) -> String {
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

fn url_decode_internal(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'%' => {
                if i + 2 >= data.len() {
                    return None;
                }
                let hi = (data[i + 1] as char).to_digit(16)?;
                let lo = (data[i + 2] as char).to_digit(16)?;
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
    Some(out)
}

// ---------- decompress helpers (magic bytes based) ----------

/// Gzip decompress (magic: 1f 8b).
fn try_gzip_decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 2 || data[0] != 0x1f || data[1] != 0x8b {
        return None;
    }
    let mut dec = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// Zlib decompress (magic: 78 01, 78 5e, 78 9c, 78 da).
fn try_zlib_decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 2 {
        return None;
    }
    let header = (data[0], data[1]);
    if !matches!(
        header,
        (0x78, 0x01) | (0x78, 0x5e) | (0x78, 0x9c) | (0x78, 0xda)
    ) {
        return None;
    }
    let mut dec = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// Bzip2 decompress (magic: 42 5a 68).
fn try_bzip2_decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 3 || data[0] != 0x42 || data[1] != 0x5a || data[2] != 0x68 {
        return None;
    }
    let mut dec = bzip2::read::BzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// LZMA decompress (magic: fd 37 7a 58 5a 00).
fn try_lzma_decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 6
        || data[0] != 0xfd
        || data[1] != 0x37
        || data[2] != 0x7a
        || data[3] != 0x58
        || data[4] != 0x5a
        || data[5] != 0x00
    {
        return None;
    }
    let mut dec = xz2::read::XzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out).ok()?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

inventory::submit! { crate::TransformEntry(&MagicWand) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn detects_base64() {
        let ctx = NullExecutionContext;
        let b64 = general_purpose::STANDARD.encode(b"Hello World");
        let out = MagicWand
            .apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.ends_with(b"Hello World"));
    }

    #[test]
    fn detects_hex() {
        let ctx = NullExecutionContext;
        let out = MagicWand
            .apply(Cow::Borrowed(b"48656c6c6f"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.ends_with(b"Hello"));
    }

    #[test]
    fn detects_base32() {
        let ctx = NullExecutionContext;
        let out = MagicWand
            .apply(Cow::Borrowed(b"JBSWY3DP"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.ends_with(b"Hello"));
    }

    #[test]
    fn detects_base58() {
        let ctx = NullExecutionContext;
        let out = MagicWand
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
        let out = MagicWand
            .apply(
                Cow::Borrowed(b"Hello%20World%21"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert!(out.ends_with(b"Hello World!"));
    }

    #[test]
    fn detects_base64_with_hex_intermediate() {
        let ctx = NullExecutionContext;
        // "test" -> hex "74657374" -> base64 "NzQ2NTc3NzQ="
        // After base64 decode, we get "74657374" which is a valid hex string.
        // After hex decode, we get "test" which is 100% printable.
        let hexed = hex::encode(b"test");
        let b64 = general_purpose::STANDARD.encode(hexed.as_bytes());
        let out = MagicWand
            .apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        // The chain should start with base64.
        assert!(out.windows(6).any(|w| w == b"base64"));
    }

    #[test]
    fn detects_base64_then_gzip() {
        let ctx = NullExecutionContext;
        // "hello" → gzip → base64
        let original = b"hello";
        let mut gz_enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz_enc, original).unwrap();
        let gz_bytes = gz_enc.finish().unwrap();
        let b64 = general_purpose::STANDARD.encode(&gz_bytes);
        let out = MagicWand
            .apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        // Should detect base64 -> gzip chain.
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.windows(4).any(|w| w == b"gzip"));
        assert!(out.ends_with(b"hello"));
    }

    #[test]
    fn detects_hex_then_base64() {
        let ctx = NullExecutionContext;
        // raw bytes → base64 → hex
        let raw = b"test data";
        let b64 = general_purpose::STANDARD.encode(raw);
        let hexed = hex::encode(b64.as_bytes());
        let out = MagicWand
            .apply(
                Cow::Borrowed(hexed.as_bytes()),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        // Should detect hex -> base64 chain.
        assert!(out.windows(3).any(|w| w == b"hex"));
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.ends_with(b"test data"));
    }

    #[test]
    fn rejects_unknown() {
        let ctx = NullExecutionContext;
        let err = MagicWand
            .apply(Cow::Borrowed(b"\x00\xFF\xFE"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn max_depth_limit() {
        let ctx = NullExecutionContext;
        // Create a deeply nested encoding: base64 applied 6 times (exceeds MAX_DEPTH=5).
        let mut data = b"deep".to_vec();
        for _ in 0..6 {
            data = general_purpose::STANDARD.encode(&data).into_bytes();
        }
        let out = MagicWand
            .apply(Cow::Borrowed(&data), &serde_json::json!({}), &ctx)
            .unwrap();
        // Should stop at MAX_DEPTH and still produce output.
        assert!(out.starts_with(b"// magic_wand:"));
        // The chain should have at most MAX_DEPTH operations.
        let chain_line = std::str::from_utf8(&out).unwrap().lines().next().unwrap();
        let ops: Vec<&str> = chain_line
            .strip_prefix("// magic_wand: ")
            .unwrap()
            .split(" -> ")
            .collect();
        assert!(ops.len() <= MAX_DEPTH, "chain too long: {:?}", ops);
    }

    #[test]
    fn no_cycles() {
        let ctx = NullExecutionContext;
        // "AA" is printable ASCII, so it's a valid final result (no decoding needed).
        // The important thing is that we terminate without infinite looping.
        let out = MagicWand
            .apply(Cow::Borrowed(b"AA"), &serde_json::json!({}), &ctx)
            .unwrap();
        // Should terminate and produce some output (either "AA" or decoded).
        assert!(out.starts_with(b"// magic_wand:"));
    }

    #[test]
    fn deterministic_output() {
        let ctx = NullExecutionContext;
        let input = b"SGVsbG8gV29ybGQ="; // base64("Hello World")
        let out1 = MagicWand
            .apply(Cow::Borrowed(input), &serde_json::json!({}), &ctx)
            .unwrap();
        let out2 = MagicWand
            .apply(Cow::Borrowed(input), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out1, out2);
    }

    #[test]
    fn detects_gzip_directly() {
        let ctx = NullExecutionContext;
        let original = b"direct gzip test";
        let mut gz_enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz_enc, original).unwrap();
        let gz_bytes = gz_enc.finish().unwrap();
        let out = MagicWand
            .apply(Cow::Borrowed(&gz_bytes), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.windows(4).any(|w| w == b"gzip"));
        assert!(out.ends_with(b"direct gzip test"));
    }

    #[test]
    fn detects_zlib_directly() {
        let ctx = NullExecutionContext;
        let original = b"direct zlib test";
        let mut zlib_enc =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut zlib_enc, original).unwrap();
        let zlib_bytes = zlib_enc.finish().unwrap();
        let out = MagicWand
            .apply(Cow::Borrowed(&zlib_bytes), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.windows(4).any(|w| w == b"zlib"));
        assert!(out.ends_with(b"direct zlib test"));
    }

    #[test]
    fn detects_bzip2_directly() {
        let ctx = NullExecutionContext;
        let original = b"direct bzip2 test";
        let mut bz_enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        std::io::Write::write_all(&mut bz_enc, original).unwrap();
        let bz_bytes = bz_enc.finish().unwrap();
        let out = MagicWand
            .apply(Cow::Borrowed(&bz_bytes), &serde_json::json!({}), &ctx)
            .unwrap();
        assert!(out.windows(5).any(|w| w == b"bzip2"));
        assert!(out.ends_with(b"direct bzip2 test"));
    }

    #[test]
    fn detects_base85() {
        let ctx = NullExecutionContext;
        // "Hello" → base85
        let encoded = base85_encode(b"Hello");
        let out = MagicWand
            .apply(
                Cow::Borrowed(encoded.as_bytes()),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert!(out.windows(6).any(|w| w == b"base85"));
        assert!(out.ends_with(b"Hello"));
    }
}
