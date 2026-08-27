pub mod blake3;
pub mod crc32;
use digest::Digest;
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use md5::Md5;
use sha2::{Sha256, Sha512};
use sha1::Sha1;
use sha3::Sha3_256;
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

pub struct Sha1Hash;

impl Transform for Sha1Hash {
    fn id(&self) -> &'static str {
        "hashing.sha1"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "SHA-1"
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
        let digest = Sha1::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
}

pub struct Sha512Hash;

impl Transform for Sha512Hash {
    fn id(&self) -> &'static str {
        "hashing.sha512"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "SHA-512"
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
        let digest = Sha512::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
}

pub struct Sha3_256Hash;

impl Transform for Sha3_256Hash {
    fn id(&self) -> &'static str {
        "hashing.sha3_256"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "SHA3-256"
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
        let digest = Sha3_256::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Md5Hash) }
inventory::submit! { crate::TransformEntry(&Sha256Hash) }
inventory::submit! { crate::TransformEntry(&Sha1Hash) }
inventory::submit! { crate::TransformEntry(&Sha512Hash) }
inventory::submit! { crate::TransformEntry(&Sha3_256Hash) }

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

    #[test]
    fn sha1_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha1Hash.apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx).unwrap();
        assert_eq!(out.as_ref(), b"a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn sha512_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha512Hash.apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx).unwrap();
        assert_eq!(
            out.as_ref(),
            b"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f" as &[u8]
        );
    }

    #[test]
    fn sha3_256_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha3_256Hash.apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx).unwrap();
        assert_eq!(
            out.as_ref(),
            b"3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532" as &[u8]
        );
    }
}
