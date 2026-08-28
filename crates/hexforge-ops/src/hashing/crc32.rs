//! `hashing.crc32` — IEEE CRC-32 (полином 0xEDB88320, отражённый),
//! классическая контрольная сумма Ethernet/zip/png (PRD §3.3 Hashing).

use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

/// Отражённый полином IEEE 802.3.
const POLYNOMIAL: u32 = 0xEDB8_8320;

/// Ленивая статическая таблица 256 записей (256 × u32 = 1 КиБ, вычисляется
/// один раз за процесс).
fn table() -> &'static [u32; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                if c & 1 != 0 {
                    c = POLYNOMIAL ^ (c >> 1);
                } else {
                    c >>= 1;
                }
            }
            *slot = c;
        }
        t
    })
}

fn crc32(data: &[u8]) -> u32 {
    let t = table();
    let mut crc = 0xFFFF_FFFF;
    for &b in data {
        crc = t[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

struct Crc32State {
    crc: u32,
}

impl Default for Crc32State {
    fn default() -> Self {
        Self { crc: 0xFFFF_FFFF }
    }
}

pub struct Crc32Hash;

impl Transform for Crc32Hash {
    fn id(&self) -> &'static str {
        "hashing.crc32"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "CRC-32"
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
        let value = crc32(input.as_ref());
        Ok(Cow::Owned(format!("{value:08x}").into_bytes()))
    }
    fn apply_chunk(
        &self,
        chunk: &[u8],
        is_last: bool,
        state: &mut Box<dyn std::any::Any + Send>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<Vec<u8>, TransformError> {
        if state.downcast_ref::<Crc32State>().is_none() {
            *state = Box::new(Crc32State::default());
        }
        let st = state.downcast_mut::<Crc32State>().expect("Crc32State seeded");
        let t = table();
        for &b in chunk {
            st.crc = t[((st.crc ^ b as u32) & 0xff) as usize] ^ (st.crc >> 8);
        }
        if is_last {
            let value = st.crc ^ 0xFFFF_FFFF;
            return Ok(format!("{value:08x}").into_bytes());
        }
        Ok(Vec::new())
    }
}

inventory::submit! { crate::TransformEntry(&Crc32Hash) }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn empty_input_zero() {
        let ctx = NullExecutionContext;
        let out = Crc32Hash.apply(Cow::Borrowed(b""), &json!({}), &ctx).unwrap();
        assert_eq!(out.as_ref(), b"00000000");
    }

    #[test]
    fn rfc3720_check_value_123456789() {
        // Канонический вектор: CRC32("123456789") = 0xCBF43926.
        let ctx = NullExecutionContext;
        let out = Crc32Hash
            .apply(Cow::Borrowed(b"123456789"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"cbf43926");
    }

    #[test]
    fn hello_world_vector() {
        let ctx = NullExecutionContext;
        let out = Crc32Hash
            .apply(Cow::Borrowed(b"Hello, World!"), &json!({}), &ctx)
            .unwrap();
        assert_eq!(out.as_ref(), b"ec4ac3d0");
    }

    #[test]
    fn different_inputs_different_hashes() {
        let ctx = NullExecutionContext;
        let a = Crc32Hash.apply(Cow::Borrowed(b"a"), &json!({}), &ctx).unwrap();
        let b = Crc32Hash.apply(Cow::Borrowed(b"b"), &json!({}), &ctx).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn chunked_matches_whole() {
        let ctx = NullExecutionContext;
        let data = b"hello world crc32 chunked test";
        let whole = Crc32Hash.apply(Cow::Borrowed(data), &json!({}), &ctx).unwrap();
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        let mut out = Vec::new();
        out.extend_from_slice(&Crc32Hash.apply_chunk(b"hello ", false, &mut state, &json!({}), &ctx).unwrap());
        out.extend_from_slice(&Crc32Hash.apply_chunk(b"world ", false, &mut state, &json!({}), &ctx).unwrap());
        out.extend_from_slice(&Crc32Hash.apply_chunk(b"crc32 chunked test", true, &mut state, &json!({}), &ctx).unwrap());
        assert_eq!(out, whole.as_ref());
    }
}
