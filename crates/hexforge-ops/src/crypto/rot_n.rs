//! `crypto.rot_n` — обобщённый ROT-N сдвиг латинских букв (PRD §3.3 Text).
//! Параметр `n` (integer, 0–25). ROT13 — частный случай при n=13.
//! Небуквенные байты проходят без изменений.

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct RotN;

fn shift_byte(b: u8, n: u8) -> u8 {
    match b {
        b'a'..=b'z' => b'a' + (b - b'a' + n) % 26,
        b'A'..=b'Z' => b'A' + (b - b'A' + n) % 26,
        other => other,
    }
}

impl Transform for RotN {
    fn id(&self) -> &'static str {
        "crypto.rot_n"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "ROT-N"
    }
    fn category(&self) -> &'static str {
        "Cryptography"
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
            "required": ["n"],
            "properties": {
                "n": { "type": "integer", "minimum": 0, "maximum": 25,
                        "description": "Shift amount" }
            }
        })
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let n = params
            .get("n")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| TransformError::InvalidParameter {
                field: "n".into(),
                reason: "integer parameter 'n' (0–25) is required".into(),
            })?;
        if n > 25 {
            return Err(TransformError::InvalidParameter {
                field: "n".into(),
                reason: format!("n must be in [0, 25], got {n}"),
            });
        }
        let n = n as u8;
        let out: Vec<u8> = input.iter().map(|&b| shift_byte(b, n)).collect();
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&RotN) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn rot_n_13_matches_rot13() {
        let ctx = NullExecutionContext;
        let out = RotN
            .apply(
                Cow::Borrowed(b"Hello World"),
                &serde_json::json!({ "n": 13 }),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"Uryyb Jbeyq");
    }

    #[test]
    fn rot_n_0_is_identity() {
        let ctx = NullExecutionContext;
        let out = RotN
            .apply(Cow::Borrowed(b"Test 123!"), &serde_json::json!({ "n": 0 }), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"Test 123!");
    }

    #[test]
    fn rot_n_25_equals_neg_1() {
        let ctx = NullExecutionContext;
        let out = RotN
            .apply(Cow::Borrowed(b"abc"), &serde_json::json!({ "n": 25 }), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"zab");
    }

    #[test]
    fn non_letters_pass_through() {
        let ctx = NullExecutionContext;
        let out = RotN
            .apply(
                Cow::Borrowed(b"a1.B_c"),
                &serde_json::json!({ "n": 2 }),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"c1.D_e");
    }

    #[test]
    fn invalid_n_rejected() {
        let ctx = NullExecutionContext;
        let err = RotN
            .apply(
                Cow::Borrowed(b"x"),
                &serde_json::json!({ "n": 26 }),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }

    #[test]
    fn missing_n_rejected() {
        let ctx = NullExecutionContext;
        let err = RotN
            .apply(Cow::Borrowed(b"x"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidParameter { .. }));
    }
}
