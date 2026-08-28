use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

fn rot13_byte(b: u8) -> u8 {
    match b {
        b'a'..=b'z' => b'a' + (b - b'a' + 13) % 26,
        b'A'..=b'Z' => b'A' + (b - b'A' + 13) % 26,
        other => other,
    }
}

pub struct Rot13;

impl Transform for Rot13 {
    fn id(&self) -> &'static str {
        "text.rot13"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "ROT13"
    }
    fn category(&self) -> &'static str {
        "Text"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: true, // байт-независимая операция, тривиально потоковая
            memory_cost: MemoryCost::PerChunk,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let out: Vec<u8> = input.iter().copied().map(rot13_byte).collect();
        Ok(Cow::Owned(out))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        _is_last: bool,
        _state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        // Байт-независимая операция: состояние между чанками не нужно.
        Ok(chunk.iter().copied().map(rot13_byte).collect())
    }
}

inventory::submit! { crate::TransformEntry(&Rot13) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn rot13_is_involution() {
        let input: ByteView = Cow::Borrowed(b"HexForge");
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});

        let once = Rot13.apply(input.clone(), &params, &ctx).unwrap();
        assert_ne!(once.as_ref(), input.as_ref());

        let twice = Rot13.apply(once, &params, &ctx).unwrap();
        assert_eq!(twice.as_ref(), input.as_ref());
    }

    #[test]
    fn chunked_matches_apply_on_split_input() {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});
        let input = b"Split Me 123";

        let whole = Rot13.apply(Cow::Borrowed(input), &params, &ctx).unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut chunked = Vec::new();
        for (i, part) in [b"Spl".as_slice(), b"it ", b"Me", b" 123"]
            .iter()
            .enumerate()
        {
            chunked.extend_from_slice(
                &Rot13
                    .apply_chunk(part, i == 3, &mut state, &params, &ctx)
                    .unwrap(),
            );
        }
        assert_eq!(whole.as_ref(), chunked.as_slice());
    }
}
