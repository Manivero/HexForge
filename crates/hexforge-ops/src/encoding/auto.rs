use base64::{engine::general_purpose, Engine as _};
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
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
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let data = input.as_ref();
        let mut tried = Vec::new();

        // Try Base64 (standard and url_safe)
        for (name, engine) in [("base64", &general_purpose::STANDARD), ("base64_url", &general_purpose::URL_SAFE)] {
            if let Ok(decoded) = engine.decode(data) {
                // Heuristic: decoded should be more printable or shorter?
                let printable = decoded.iter().filter(|b| (0x20..=0x7e).contains(*b) || **b == b'\n' || **b == b'\r' || **b == b'\t').count() as f64 / decoded.len().max(1) as f64;
                if printable > 0.7 || decoded.len() < data.len() {
                    // Return decoded with annotation
                    let mut out = format!("// detected: {name}\n").into_bytes();
                    out.extend_from_slice(&decoded);
                    return Ok(Cow::Owned(out));
                }
            }
            tried.push(name);
        }
        // Try Hex
        let cleaned: String = data.iter().filter(|b| !b.is_ascii_whitespace()).map(|&b| b as char).collect();
        if cleaned.len().is_multiple_of(2) && !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(decoded) = hex::decode(&cleaned) {
                let mut out = b"// detected: hex\n".to_vec();
                out.extend_from_slice(&decoded);
                return Ok(Cow::Owned(out));
            }
        }
        tried.push("hex");

        // Try Base32
        let b32_clean: String = data.iter().filter(|b| !b.is_ascii_whitespace() && **b != b'=').map(|&b| b as char).collect();
        if !b32_clean.is_empty() && b32_clean.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7' | 'a'..='z')) {
            // Placeholder for base32 detection — not yet implemented
        }
        tried.push("base32");

        Err(TransformError::InvalidInput { reason: format!("Magic Wand could not auto-detect encoding (tried: {})", tried.join(", ")) })
    }
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
        let out = AutoDecode.apply(Cow::Borrowed(b64.as_bytes()), &serde_json::json!({}), &ctx).unwrap();
        assert!(out.windows(6).any(|w| w == b"base64"));
        assert!(out.ends_with(b"Hello World"));
    }

    #[test]
    fn detects_hex() {
        let ctx = NullExecutionContext;
        let out = AutoDecode.apply(Cow::Borrowed(b"48656c6c6f"), &serde_json::json!({}), &ctx).unwrap();
        assert!(out.ends_with(b"Hello"));
    }

    #[test]
    fn rejects_unknown() {
        let ctx = NullExecutionContext;
        let err = AutoDecode.apply(Cow::Borrowed(b"\x00\xFF\xFE"), &serde_json::json!({}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
