use digest::Digest;
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use md5::Md5;
use sha2::Sha256;
use std::borrow::Cow;

pub struct Md5Hash;

impl Transform for Md5Hash {
    fn id(&self) -> &'static str {
        "hashing.md5"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "MD5"
    }
    fn category(&self) -> &'static str {
        "Hashing"
    }
    fn capabilities(&self) -> TransformCapabilities {
        // Потоковый API у Digest-трейта есть (update() по чанкам), но MVP
        // регистрируем как non-streaming ради простоты; апгрейд до streamable
        // не меняет сигнатуру apply, только добавляет apply_chunk.
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::Constant,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let digest = Md5::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
}

pub struct Sha256Hash;

impl Transform for Sha256Hash {
    fn id(&self) -> &'static str {
        "hashing.sha256"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "SHA-256"
    }
    fn category(&self) -> &'static str {
        "Hashing"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::Constant,
        }
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let digest = Sha256::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Md5Hash) }
inventory::submit! { crate::TransformEntry(&Sha256Hash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn sha256_known_vector() {
        let input: ByteView = Cow::Borrowed(b"");
        let ctx = NullExecutionContext;
        let out = Sha256Hash.apply(input, &serde_json::json!({}), &ctx).unwrap();
        assert_eq!(
            out.as_ref(),
            b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" as &[u8]
        );
    }
}
