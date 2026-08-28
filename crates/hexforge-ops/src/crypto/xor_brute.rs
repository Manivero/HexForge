use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

pub struct XorBruteForce;

impl Transform for XorBruteForce {
    fn id(&self) -> &'static str {
        "crypto.xor_bruteforce"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "XOR Brute Force (single-byte)"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "printableOnly": { "type": "boolean", "default": true, "description": "Only show candidates with printable ASCII" }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let printable_only = params.get("printableOnly").and_then(|v| v.as_bool()).unwrap_or(true);
        let data = input.as_ref();
        if data.is_empty() {
            return Ok(Cow::Owned(Vec::new()));
        }
        let mut out = String::new();
        for key in 1u8..=255 {
            let decoded: Vec<u8> = data.iter().map(|b| b ^ key).collect();
            let is_printable = decoded.iter().all(|&b| (0x20..=0x7e).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t');
            if printable_only && !is_printable {
                continue;
            }
            let lossy = String::from_utf8_lossy(&decoded);
            out.push_str(&format!("{:02x}: {}\n", key, lossy));
        }
        if out.is_empty() {
            out.push_str("(no printable candidates)\n");
        }
        Ok(Cow::Owned(out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&XorBruteForce) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn brute_force_finds_key() {
        let ctx = NullExecutionContext;
        // plaintext "Hello" xor 0x2a -> ciphertext
        let key = 0x2a;
        let pt = b"Hello";
        let ct: Vec<u8> = pt.iter().map(|b| b ^ key).collect();
        let out = XorBruteForce.apply(Cow::Borrowed(&ct), &serde_json::json!({}), &ctx).unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains("2a: Hello"), "should contain key 2a -> Hello, got {s}");
    }

    #[test]
    fn empty_input_empty_output() {
        let ctx = NullExecutionContext;
        let out = XorBruteForce.apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn printable_filter() {
        let ctx = NullExecutionContext;
        let ct = vec![0x01, 0x02];
        let out = XorBruteForce.apply(Cow::Borrowed(&ct), &serde_json::json!({"printableOnly": true}), &ctx).unwrap();
        let len_filtered = out.len();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(!s.is_empty());
        let out_all = XorBruteForce.apply(Cow::Borrowed(&ct), &serde_json::json!({"printableOnly": false}), &ctx).unwrap();
        assert!(out_all.len() > len_filtered);
    }
}
