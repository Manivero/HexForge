use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
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
}
