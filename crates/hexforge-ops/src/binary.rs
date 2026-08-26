//! `binary.strings_extract` и `binary.entropy` — утилиты бинарного анализа
//! для Malware Analyst / DFIR (PRD §3.3 "Бинарный анализ").

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

// ---------- strings_extract ----------

pub struct StringsExtract;

impl Transform for StringsExtract {
    fn id(&self) -> &'static str {
        "binary.strings_extract"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Extract Strings"
    }
    fn category(&self) -> &'static str {
        "Binary Analysis"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "min_length": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 4,
                    "description": "Minimum printable sequence length"
                }
            }
        })
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let min_length = params
            .get("min_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(4)
            .max(1) as usize;

        let mut out = String::new();
        let mut current = String::new();

        for &b in input.as_ref() {
            if (0x20..=0x7e).contains(&b) {
                current.push(b as char);
            } else {
                if current.len() >= min_length {
                    out.push_str(&current);
                    out.push('\n');
                }
                current.clear();
            }
        }
        // Хвостовая последовательность
        if current.len() >= min_length {
            out.push_str(&current);
            out.push('\n');
        }

        Ok(Cow::Owned(out.into_bytes()))
    }
}

// ---------- entropy ----------

pub struct EntropyCalc;

impl Transform for EntropyCalc {
    fn id(&self) -> &'static str {
        "binary.entropy"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Shannon Entropy"
    }
    fn category(&self) -> &'static str {
        "Binary Analysis"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false, // MVP: весь буфер; per-chunk граф — post-MVP
            memory_cost: MemoryCost::Constant,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let data = input.as_ref();
        if data.is_empty() {
            return Ok(Cow::Owned(b"0.0000".to_vec()));
        }

        // Частотный анализ 256 байтовых значений.
        let mut freq = [0u64; 256];
        for &b in data {
            freq[b as usize] += 1;
        }

        let total = data.len() as f64;
        let entropy: f64 = freq
            .iter()
            .filter(|&&f| f > 0)
            .map(|&f| {
                let p = f as f64 / total;
                -p * p.log2()
            })
            .sum();

        // Энтропия Шеннона всегда ≥ 0; clamp устраняет возможный -0.0
        // от операций с плавающей точкой.
        let entropy = entropy.max(0.0);
        Ok(Cow::Owned(format!("{entropy:.4}").into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&StringsExtract) }
inventory::submit! { crate::TransformEntry(&EntropyCalc) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn strings_extracts_printable_sequences() {
        let ctx = NullExecutionContext;
        let data = b"ABCD\x00\x01EF\x02GHIJ";
        let out = StringsExtract
            .apply(Cow::Borrowed(data), &json!({ "min_length": 2 }), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"ABCD\nEF\nGHIJ\n");
    }

    #[test]
    fn strings_filters_by_min_length() {
        let ctx = NullExecutionContext;
        let data = b"AB\x00CD\x00EFGH";
        let out = StringsExtract
            .apply(Cow::Borrowed(data), &json!({ "min_length": 3 }), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"EFGH\n");
    }

    #[test]
    fn strings_empty_input_empty_output() {
        let ctx = NullExecutionContext;
        let out = StringsExtract
            .apply(Cow::Borrowed(b""), &json!({}), &ctx)
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn entropy_zero_for_uniform_bytes() {
        // Все байты одинаковые → энтропия 0.
        let ctx = NullExecutionContext;
        let out = EntropyCalc
            .apply(Cow::Borrowed(&[0xAA; 100]), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"0.0000");
    }

    #[test]
    fn entropy_maximal_for_uniform_distribution() {
        // Ровно по одному вхождению каждого из 256 байтов → максимальная энтропия.
        let ctx = NullExecutionContext;
        let data: Vec<u8> = (0..=255u8).collect();
        let out = EntropyCalc
            .apply(Cow::Borrowed(&data), &json!({}), &ctx)
            .unwrap();
        let value = std::str::from_utf8(out.as_ref()).unwrap().parse::<f64>().unwrap();
        assert!(
            (value - 8.0).abs() < 0.001,
            "expected ~8.0, got {value}"
        );
    }

    #[test]
    fn entropy_text_is_low_binary_is_high() {
        let ctx = NullExecutionContext;
        let text_out = EntropyCalc
            .apply(Cow::Borrowed(b"AAAAAAAABBBBBBBB"), &json!({}), &ctx)
            .unwrap();
        let binary_out = EntropyCalc
            .apply(
                Cow::Borrowed(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]),
                &json!({}),
                &ctx,
            )
            .unwrap();
        let text_val: f64 = std::str::from_utf8(text_out.as_ref()).unwrap().parse().unwrap();
        let bin_val: f64 = std::str::from_utf8(binary_out.as_ref()).unwrap().parse().unwrap();
        assert!(text_val < bin_val, "text entropy ({text_val}) < binary ({bin_val})");
    }

    #[test]
    fn entropy_empty_input_zero() {
        let ctx = NullExecutionContext;
        let out = EntropyCalc.apply(Cow::Borrowed(b""), &json!({}), &ctx).unwrap();
        assert_eq!(out.as_ref(), b"0.0000");
    }
}
