//! `compression.*` — сжатие/разжатие (PRD §3.3 Compression).
//! gzip/zlib/deflate via `flate2`, bzip2 via `bzip2`, lzma/xz via `xz2`.

use bzip2::read::{BzDecoder, BzEncoder};
use bzip2::Compression as BzCompression;
use flate2::read::{DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder};
use flate2::Compression;
use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;
use std::io::Read;
use xz2::read::{XzDecoder, XzEncoder};

// ---------- helpers ----------

fn compress_bytes(encoder: &mut dyn Read) -> Result<Vec<u8>, TransformError> {
    let mut out = Vec::new();
    encoder
        .read_to_end(&mut out)
        .map_err(|e| TransformError::Internal(format!("compression failed: {e}")))?;
    Ok(out)
}

fn decompress_bytes(decoder: &mut dyn Read) -> Result<Vec<u8>, TransformError> {
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| TransformError::InvalidInput {
            reason: format!("decompression failed: {e}"),
        })?;
    Ok(out)
}

fn level_from_params(params: &serde_json::Value) -> Compression {
    let lvl = params
        .get("level")
        .and_then(|v| v.as_u64())
        .unwrap_or(6);
    // flate2 level 0..=9; clamp to valid range
    Compression::new((lvl.min(9)) as u32)
}

// ---------- gzip ----------

pub struct GzipCompress;

impl Transform for GzipCompress {
    fn id(&self) -> &'static str {
        "compression.gzip.compress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Gzip Compress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "minimum": 0, "maximum": 9, "default": 6 }
            }
        })
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
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut enc = GzEncoder::new(input.as_ref(), level_from_params(params));
        Ok(Cow::Owned(compress_bytes(&mut enc)?))
    }
}

pub struct GzipDecompress;

impl Transform for GzipDecompress {
    fn id(&self) -> &'static str {
        "compression.gzip.decompress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Gzip Decompress"
    }
    fn category(&self) -> &'static str {
        "Compression"
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
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut dec = GzDecoder::new(input.as_ref());
        Ok(Cow::Owned(decompress_bytes(&mut dec)?))
    }
}

// ---------- zlib ----------

pub struct ZlibCompress;

impl Transform for ZlibCompress {
    fn id(&self) -> &'static str {
        "compression.zlib.compress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Zlib Compress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "minimum": 0, "maximum": 9, "default": 6 }
            }
        })
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
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut enc = ZlibEncoder::new(input.as_ref(), level_from_params(params));
        Ok(Cow::Owned(compress_bytes(&mut enc)?))
    }
}

pub struct ZlibDecompress;

impl Transform for ZlibDecompress {
    fn id(&self) -> &'static str {
        "compression.zlib.decompress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Zlib Decompress"
    }
    fn category(&self) -> &'static str {
        "Compression"
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
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut dec = ZlibDecoder::new(input.as_ref());
        Ok(Cow::Owned(decompress_bytes(&mut dec)?))
    }
}

// ---------- deflate (raw RFC1951) ----------

pub struct DeflateCompress;

impl Transform for DeflateCompress {
    fn id(&self) -> &'static str {
        "compression.deflate.compress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Deflate Compress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "minimum": 0, "maximum": 9, "default": 6 }
            }
        })
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
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut enc = DeflateEncoder::new(input.as_ref(), level_from_params(params));
        Ok(Cow::Owned(compress_bytes(&mut enc)?))
    }
}

pub struct DeflateDecompress;

impl Transform for DeflateDecompress {
    fn id(&self) -> &'static str {
        "compression.deflate.decompress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Deflate Decompress"
    }
    fn category(&self) -> &'static str {
        "Compression"
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
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut dec = DeflateDecoder::new(input.as_ref());
        Ok(Cow::Owned(decompress_bytes(&mut dec)?))
    }
}

pub struct Bzip2Compress;

impl Transform for Bzip2Compress {
    fn id(&self) -> &'static str {
        "compression.bzip2.compress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Bzip2 Compress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "minimum": 1, "maximum": 9, "default": 6 }
            }
        })
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
        input: ByteView<'a>,
        params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let lvl = params.get("level").and_then(|v| v.as_u64()).unwrap_or(6).clamp(1, 9) as u32;
        let mut enc = BzEncoder::new(input.as_ref(), BzCompression::new(lvl));
        Ok(Cow::Owned(compress_bytes(&mut enc)?))
    }
}

pub struct Bzip2Decompress;

impl Transform for Bzip2Decompress {
    fn id(&self) -> &'static str {
        "compression.bzip2.decompress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "Bzip2 Decompress"
    }
    fn category(&self) -> &'static str {
        "Compression"
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
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let mut dec = BzDecoder::new(input.as_ref());
        Ok(Cow::Owned(decompress_bytes(&mut dec)?))
    }
}

pub struct LzmaCompress;

impl Transform for LzmaCompress {
    fn id(&self) -> &'static str {
        "compression.lzma.compress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "LZMA Compress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer", "minimum": 0, "maximum": 9, "default": 6 }
            }
        })
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let lvl = params.get("level").and_then(|v| v.as_u64()).unwrap_or(6).clamp(0, 9) as u32;
        let mut enc = XzEncoder::new(input.as_ref(), lvl);
        Ok(Cow::Owned(compress_bytes(&mut enc)?))
    }
}

pub struct LzmaDecompress;

impl Transform for LzmaDecompress {
    fn id(&self) -> &'static str {
        "compression.lzma.decompress"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "LZMA Decompress"
    }
    fn category(&self) -> &'static str {
        "Compression"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let mut dec = XzDecoder::new(input.as_ref());
        Ok(Cow::Owned(decompress_bytes(&mut dec)?))
    }
}

inventory::submit! { crate::TransformEntry(&GzipCompress) }
inventory::submit! { crate::TransformEntry(&GzipDecompress) }
inventory::submit! { crate::TransformEntry(&ZlibCompress) }
inventory::submit! { crate::TransformEntry(&ZlibDecompress) }
inventory::submit! { crate::TransformEntry(&DeflateCompress) }
inventory::submit! { crate::TransformEntry(&DeflateDecompress) }
inventory::submit! { crate::TransformEntry(&Bzip2Compress) }
inventory::submit! { crate::TransformEntry(&Bzip2Decompress) }
inventory::submit! { crate::TransformEntry(&LzmaCompress) }
inventory::submit! { crate::TransformEntry(&LzmaDecompress) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    fn roundtrip(compress: &dyn Transform, decompress: &dyn Transform, data: &[u8]) {
        let ctx = NullExecutionContext;
        let params = serde_json::json!({});
        let enc = compress.apply(Cow::Borrowed(data), &params, &ctx).unwrap();
        assert_ne!(enc.as_ref(), data, "compressed must differ for non-empty input");
        let dec = decompress.apply(enc, &params, &ctx).unwrap();
        assert_eq!(dec.as_ref(), data);
    }

    #[test]
    fn gzip_roundtrip() {
        roundtrip(&GzipCompress, &GzipDecompress, b"Hello HexForge gzip!");
        roundtrip(&GzipCompress, &GzipDecompress, b"");
        let big = vec![b'A'; 100_000];
        roundtrip(&GzipCompress, &GzipDecompress, &big);
    }

    #[test]
    fn zlib_roundtrip() {
        roundtrip(&ZlibCompress, &ZlibDecompress, b"The quick brown fox jumps");
        roundtrip(&ZlibCompress, &ZlibDecompress, b"");
    }

    #[test]
    fn deflate_roundtrip() {
        roundtrip(&DeflateCompress, &DeflateDecompress, b"deflate test payload 123");
    }

    #[test]
    fn decompress_rejects_invalid_input() {
        let ctx = NullExecutionContext;
        let err = GzipDecompress
            .apply(Cow::Borrowed(b"not gzipped"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
        let err2 = ZlibDecompress
            .apply(Cow::Borrowed(b"bad zlib"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err2, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn level_param_accepted() {
        let ctx = NullExecutionContext;
        for lvl in [0u64, 1, 6, 9] {
            let params = serde_json::json!({ "level": lvl });
            let enc = GzipCompress
                .apply(Cow::Borrowed(b"level test"), &params, &ctx)
                .unwrap();
            let dec = GzipDecompress.apply(enc, &serde_json::json!({}), &ctx).unwrap();
            assert_eq!(dec.as_ref(), b"level test");
        }
    }

    #[test]
    fn bzip2_roundtrip() {
        roundtrip(&Bzip2Compress, &Bzip2Decompress, b"bzip2 payload test");
        roundtrip(&Bzip2Compress, &Bzip2Decompress, b"");
        let big = vec![b'X'; 50_000];
        roundtrip(&Bzip2Compress, &Bzip2Decompress, &big);
    }

    #[test]
    fn lzma_roundtrip() {
        roundtrip(&LzmaCompress, &LzmaDecompress, b"lzma payload test for HexForge");
        roundtrip(&LzmaCompress, &LzmaDecompress, b"");
        let big = vec![b'Y'; 50_000];
        roundtrip(&LzmaCompress, &LzmaDecompress, &big);
    }
}
