pub mod argon2;
pub mod blake3;
pub mod crc32;
pub mod hmac;
pub mod pbkdf2;
pub mod scrypt;
use blake2::{Blake2b512, Blake2s256};
use digest::Digest;
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
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
        let digest = Md5::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Md5>().is_none() {
            *state = Box::new(Md5::new());
        }
        let h = state.downcast_mut::<Md5>().expect("Md5 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
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
        let digest = Sha256::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Sha256>().is_none() {
            *state = Box::new(Sha256::new());
        }
        let h = state.downcast_mut::<Sha256>().expect("Sha256 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
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
        let digest = Sha1::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Sha1>().is_none() {
            *state = Box::new(Sha1::new());
        }
        let h = state.downcast_mut::<Sha1>().expect("Sha1 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
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
        let digest = Sha512::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Sha512>().is_none() {
            *state = Box::new(Sha512::new());
        }
        let h = state.downcast_mut::<Sha512>().expect("Sha512 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
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
        let digest = Sha3_256::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Sha3_256>().is_none() {
            *state = Box::new(Sha3_256::new());
        }
        let h = state.downcast_mut::<Sha3_256>().expect("Sha3_256 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
    }
}

pub struct Blake2bHash;

impl Transform for Blake2bHash {
    fn id(&self) -> &'static str {
        "hashing.blake2b"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "BLAKE2b-512"
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
        let digest = Blake2b512::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Blake2b512>().is_none() {
            *state = Box::new(Blake2b512::new());
        }
        let h = state
            .downcast_mut::<Blake2b512>()
            .expect("Blake2b512 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
    }
}

pub struct Blake2sHash;

impl Transform for Blake2sHash {
    fn id(&self) -> &'static str {
        "hashing.blake2s"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "BLAKE2s-256"
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
        let digest = Blake2s256::digest(input.as_ref());
        Ok(Cow::Owned(hex::encode(digest).into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Blake2s256>().is_none() {
            *state = Box::new(Blake2s256::new());
        }
        let h = state
            .downcast_mut::<Blake2s256>()
            .expect("Blake2s256 seeded");
        h.update(chunk);
        if is_last {
            let out = h.clone().finalize();
            return Ok(hex::encode(out).into_bytes());
        }
        Ok(Vec::new())
    }
}

pub struct SsdeepHash;

impl Transform for SsdeepHash {
    fn id(&self) -> &'static str {
        "hashing.ssdeep"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "SSDEEP"
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
        let hash = fuzzyhash::FuzzyHash::new(input.as_ref()).to_string();
        Ok(Cow::Owned(hash.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&Md5Hash) }
inventory::submit! { crate::TransformEntry(&Sha256Hash) }
inventory::submit! { crate::TransformEntry(&Sha1Hash) }
inventory::submit! { crate::TransformEntry(&Sha512Hash) }
inventory::submit! { crate::TransformEntry(&Sha3_256Hash) }
inventory::submit! { crate::TransformEntry(&Blake2bHash) }
inventory::submit! { crate::TransformEntry(&Blake2sHash) }
inventory::submit! { crate::TransformEntry(&SsdeepHash) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn sha256_known_vector() {
        let input: ByteView = Cow::Borrowed(b"");
        let ctx = NullExecutionContext;
        let out = Sha256Hash
            .apply(input, &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" as &[u8]
        );
    }

    #[test]
    fn sha1_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha1Hash
            .apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn sha512_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha512Hash
            .apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f" as &[u8]
        );
    }

    #[test]
    fn sha3_256_known_vector() {
        let ctx = NullExecutionContext;
        let out = Sha3_256Hash
            .apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(
            out.as_ref(),
            b"3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532" as &[u8]
        );
    }

    #[test]
    fn blake2b_known_vector() {
        let ctx = NullExecutionContext;
        let out = Blake2bHash
            .apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce");
    }

    #[test]
    fn blake2s_known_vector() {
        let ctx = NullExecutionContext;
        let out = Blake2sHash
            .apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(out.len(), 64, "blake2s hex len 64");
        let out2 = Blake2sHash
            .apply(Cow::Borrowed(b"abc"), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_ne!(out, out2);
        assert_eq!(out2.len(), 64);
    }

    #[test]
    fn ssdeep_hash() {
        let ctx = NullExecutionContext;
        let out = SsdeepHash
            .apply(
                Cow::Borrowed(b"Hello ssdeep test input for fuzzy hashing"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains(':'), "ssdeep format blocksize:hash:hash");
        assert!(s.len() > 10);
    }

    #[test]
    fn sha256_chunked_matches_whole() {
        let ctx = NullExecutionContext;
        let data = b"hello world chunked hashing test";
        let whole = Sha256Hash
            .apply(Cow::Borrowed(data), &serde_json::json!({}), &ctx)
            .unwrap();
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut out = Vec::new();
        out.extend_from_slice(
            &Sha256Hash
                .apply_chunk(b"hello ", false, &mut state, &serde_json::json!({}), &ctx)
                .unwrap(),
        );
        out.extend_from_slice(
            &Sha256Hash
                .apply_chunk(b"world ", false, &mut state, &serde_json::json!({}), &ctx)
                .unwrap(),
        );
        out.extend_from_slice(
            &Sha256Hash
                .apply_chunk(
                    b"chunked hashing test",
                    true,
                    &mut state,
                    &serde_json::json!({}),
                    &ctx,
                )
                .unwrap(),
        );
        assert_eq!(out, whole.as_ref());
    }

    #[test]
    fn md5_chunked_matches_whole() {
        let ctx = NullExecutionContext;
        let data = b"md5 chunked";
        let whole = Md5Hash
            .apply(Cow::Borrowed(data), &serde_json::json!({}), &ctx)
            .unwrap();
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut out = Vec::new();
        out.extend_from_slice(
            &Md5Hash
                .apply_chunk(b"md5 ", false, &mut state, &serde_json::json!({}), &ctx)
                .unwrap(),
        );
        out.extend_from_slice(
            &Md5Hash
                .apply_chunk(b"chunked", true, &mut state, &serde_json::json!({}), &ctx)
                .unwrap(),
        );
        assert_eq!(out, whole.as_ref());
    }
}
