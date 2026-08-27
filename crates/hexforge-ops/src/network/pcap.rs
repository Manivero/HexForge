use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

pub struct PcapInfo;

impl Transform for PcapInfo {
    fn id(&self) -> &'static str {
        "network.pcap_info"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "PCAP Info"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer }
    }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let data = input.as_ref();
        if data.len() < 24 {
            return Err(TransformError::InvalidInput { reason: "too short for PCAP global header (need 24 bytes)".into() });
        }
        // Determine endianness via magic
        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let le = match magic {
            0xd4c3b2a1 => true,  // little endian
            0xa1b2c3d4 => false, // big endian
            _ => return Err(TransformError::InvalidInput { reason: format!("invalid PCAP magic 0x{magic:08x}") }),
        };
        let read_u32 = |off: usize| -> u32 {
            let b = [data[off], data[off+1], data[off+2], data[off+3]];
            if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
        };
        let version_major = if le {
            u16::from_le_bytes([data[4], data[5]])
        } else {
            u16::from_be_bytes([data[4], data[5]])
        };
        let _version_minor = if le {
            u16::from_le_bytes([data[6], data[7]])
        } else {
            u16::from_be_bytes([data[6], data[7]])
        };
        // For simplicity, just parse packet count
        let mut offset = 24usize;
        let mut packet_count = 0usize;
        let mut total_incl = 0u64;
        let mut max_incl = 0u32;
        while offset + 16 <= data.len() {
            let incl_len = read_u32(offset + 8);
            let _orig_len = read_u32(offset + 12);
            if incl_len > 65535 * 4 {
                // Likely corrupt (incl_len huge)
                break;
            }
            if offset + 16 + incl_len as usize > data.len() {
                break;
            }
            packet_count += 1;
            total_incl += incl_len as u64;
            if incl_len > max_incl { max_incl = incl_len; }
            offset += 16 + incl_len as usize;
        }
        let out = serde_json::json!({
            "endian": if le { "little" } else { "big" },
            "version_major": version_major,
            "packet_count": packet_count,
            "total_bytes": total_incl,
            "max_packet_size": max_incl,
        });
        let pretty = serde_json::to_string_pretty(&out).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&PcapInfo) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    fn make_pcap(packet_data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        // little endian magic d4 c3 b2 a1 + version 2.4 + thiszone 0 + sigfigs 0 + snaplen 65535 + network 1
        v.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&0i32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&65535u32.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        // packet header: ts_sec 0, ts_usec 0, incl_len, orig_len
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&(packet_data.len() as u32).to_le_bytes());
        v.extend_from_slice(&(packet_data.len() as u32).to_le_bytes());
        v.extend_from_slice(packet_data);
        v
    }

    #[test]
    fn parses_single_packet() {
        let ctx = NullExecutionContext;
        let pcap = make_pcap(b"hello");
        let out = PcapInfo.apply(Cow::Borrowed(&pcap), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["packet_count"], 1);
        assert_eq!(v["endian"], "little");
    }

    #[test]
    fn rejects_invalid_magic() {
        let ctx = NullExecutionContext;
        let err = PcapInfo.apply(Cow::Borrowed(b"not a pcap"), &serde_json::json!({}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn empty_pcap_no_packets() {
        let ctx = NullExecutionContext;
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
        hdr.extend_from_slice(&2u16.to_le_bytes());
        hdr.extend_from_slice(&4u16.to_le_bytes());
        hdr.extend_from_slice(&0i32.to_le_bytes());
        hdr.extend_from_slice(&0u32.to_le_bytes());
        hdr.extend_from_slice(&65535u32.to_le_bytes());
        hdr.extend_from_slice(&1u32.to_le_bytes());
        let out = PcapInfo.apply(Cow::Borrowed(&hdr), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["packet_count"], 0);
    }
}
