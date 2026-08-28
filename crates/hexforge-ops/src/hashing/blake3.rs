//! `hashing.blake3` — BLAKE3 криптографический хэш (256 бит, hex).
//! blake3 уже является workspace-зависимостью через hexforge-core.

use blake3::Hasher;
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct Blake3Hash;

impl Transform for Blake3Hash {
    fn id(&self) -> &'static str {
        "hashing.blake3"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "BLAKE3"
    }
    fn category(&self) -> &'static str {
        "Hashing"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: true,
            memory_cost: MemoryCost::Constant,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut hasher = Hasher::new();
        hasher.update(input.as_ref());
        let hash = hasher.finalize();
        Ok(Cow::Owned(hash.to_hex().to_string().into_bytes()))
    }

    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        // BLAKE3 streaming: хэшируем чанки инкрементально в state.
        // На последнем чанке финализируем и возвращаем hex-хэш.
        if let Some(hasher) = state.downcast_mut::<Hasher>() {
            hasher.update(chunk);
            if is_last {
                let hash = hasher.finalize();
                return Ok(hash.to_hex().to_string().into_bytes());
            }
            return Ok(Vec::new());
        }
        // Первый чанк: засеиваем hasher.
        let mut h = Hasher::new();
        h.update(chunk);
        if is_last {
            let hash = h.finalize();
            return Ok(hash.to_hex().to_string().into_bytes());
        }
        *state = Box::new(h);
        Ok(Vec::new())
    }
}

inventory::submit! { crate::TransformEntry(&Blake3Hash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;
    use serde_json::json;

    #[test]
    fn empty_input_known_vector() {
        // BLAKE3("") = af1349b9f5f9a1a6a0404dea36dcc949...
        let ctx = NullExecutionContext;
        let out = Blake3Hash
            .apply(Cow::Borrowed(b""), &json!({}), &ctx)
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn known_vector_abc() {
        // BLAKE3("abc") = 6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85
        let ctx = NullExecutionContext;
        let out = Blake3Hash
            .apply(Cow::Borrowed(b"abc"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }

    #[test]
    fn chunked_matches_whole() {
        // Стриминговое хэширование по частям должно давать тот же результат,
        // что и разовое apply. apply_chunk накапливает состояние и выдаёт
        // hex-хэш на последнем чанке (is_last=true).
        let ctx = NullExecutionContext;

        let expected = Blake3Hash
            .apply(Cow::Borrowed(b"hello world"), &json!({}), &ctx)
            .unwrap();

        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        Blake3Hash
            .apply_chunk(b"hell", false, &mut state, &json!({}), &ctx)
            .unwrap();
        Blake3Hash
            .apply_chunk(b"o wor", false, &mut state, &json!({}), &ctx)
            .unwrap();
        let final_chunk = Blake3Hash
            .apply_chunk(b"ld", true, &mut state, &json!({}), &ctx)
            .unwrap();

        // Последний чанк возвращает полный hex-хэш.
        assert_eq!(final_chunk.as_slice(), expected.as_ref());
    }
}
