use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, MergeTransform, Transform, TransformCapabilities,
    TransformError,
};
use std::borrow::Cow;

pub struct DiffMerge;

impl Transform for DiffMerge {
    fn id(&self) -> &'static str {
        "streaming.diff"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Diff Inputs"
    }
    fn category(&self) -> &'static str {
        "Streaming"
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
        _input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        Err(TransformError::Internal(
            "streaming.diff is a merge operation; use apply_merge".into(),
        ))
    }
}

impl MergeTransform for DiffMerge {
    fn apply_merge<'a>(
        &self,
        inputs: Vec<ByteView<'a>>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        if inputs.len() != 2 {
            return Err(TransformError::InvalidInput {
                reason: format!(
                    "streaming.diff requires exactly 2 inputs, got {}",
                    inputs.len()
                ),
            });
        }
        let a = inputs[0].as_ref();
        let b = inputs[1].as_ref();
        if a == b {
            return Ok(Cow::Owned(b"equal\n".to_vec()));
        }
        let mut out = String::new();
        let max_len = a.len().max(b.len());
        let mut diffs = 0usize;
        for i in 0..max_len {
            let av = a.get(i).copied();
            let bv = b.get(i).copied();
            if av != bv {
                diffs += 1;
                if diffs <= 32 {
                    out.push_str(&format!(
                        "offset 0x{:08x}: {:02x?} != {:02x?}\n",
                        i,
                        av.map(|v| format!("{v:02x}"))
                            .unwrap_or_else(|| "--".into()),
                        bv.map(|v| format!("{v:02x}"))
                            .unwrap_or_else(|| "--".into())
                    ));
                }
                if diffs == 33 {
                    out.push_str("... truncated\n");
                }
            }
        }
        out.push_str(&format!(
            "\ntotal diff bytes: {diffs} / {max_len} ({:.2}%)\n",
            (diffs as f64 / max_len as f64) * 100.0
        ));
        if a.len() != b.len() {
            out.push_str(&format!("length diff: {} vs {}\n", a.len(), b.len()));
        }
        Ok(Cow::Owned(out.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&DiffMerge) }
inventory::submit! { crate::MergeEntry(&DiffMerge) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn equal_inputs() {
        let ctx = NullExecutionContext;
        let out = DiffMerge
            .apply_merge(
                vec![Cow::Borrowed(b"abc"), Cow::Borrowed(b"abc")],
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        assert_eq!(out.as_ref(), b"equal\n");
    }

    #[test]
    fn finds_diff() {
        let ctx = NullExecutionContext;
        let out = DiffMerge
            .apply_merge(
                vec![Cow::Borrowed(b"abc"), Cow::Borrowed(b"abd")],
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains("offset 0x00000002"));
        assert!(s.contains("total diff bytes: 1"));
    }

    #[test]
    fn length_diff() {
        let ctx = NullExecutionContext;
        let out = DiffMerge
            .apply_merge(
                vec![Cow::Borrowed(b"a"), Cow::Borrowed(b"ab")],
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains("length diff"));
    }

    #[test]
    fn requires_two_inputs() {
        let ctx = NullExecutionContext;
        let err = DiffMerge
            .apply_merge(vec![Cow::Borrowed(b"a")], &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
