use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

pub struct PcapParse;

impl Transform for PcapParse {
    fn id(&self) -> &'static str {
        "network.pcap_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "PCAP Parse"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities { TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer } }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let data = input.as_ref();
        if data.len() < 24 { return Err(TransformError::InvalidInput { reason: "too short for PCAP global header".into() }); }
        let magic_be = u32::from_be_bytes([data[0],data[1],data[2],data[3]]);
        let le = match magic_be {
            0xd4c3b2a1 => true,
            0xa1b2c3d4 => false,
            _ => return Err(TransformError::InvalidInput { reason: format!("invalid PCAP magic 0x{magic_be:08x}") }),
        };
        let read_u32 = |off: usize| -> u32 {
            let b = [data[off], data[off+1], data[off+2], data[off+3]];
            if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
        };
        // Global header: we only need linktype at offset 20 (network)
        let linktype = read_u32(20);
        let mut offset = 24usize;
        let mut packets = Vec::new();
        let mut idx = 0usize;
        while offset + 16 <= data.len() && packets.len() < 64 {
            let ts_sec = read_u32(offset);
            let incl_len = read_u32(offset+8) as usize;
            if offset + 16 + incl_len > data.len() { break; }
            let pkt_data = &data[offset+16 .. offset+16+incl_len];
            let mut info = serde_json::json!({ "index": idx, "ts_sec": ts_sec, "incl_len": incl_len, "linktype": linktype });
            if linktype == 1 && pkt_data.len() >= 14 {
                // Ethernet — ethertype is always BE regardless of pcap endian
                let ethertype_be = u16::from_be_bytes([pkt_data[12], pkt_data[13]]);
                info["ethertype"] = serde_json::json!(format!("0x{ethertype_be:04x}"));
                if ethertype_be == 0x0800 && pkt_data.len() >= 34 {
                    // IPv4
                    let ip_start = 14;
                    let ihl = (pkt_data[ip_start] & 0x0F) as usize * 4;
                    if pkt_data.len() >= ip_start + 20 {
                        let proto = pkt_data[ip_start+9];
                        let src_ip = format!("{}.{}.{}.{}", pkt_data[ip_start+12], pkt_data[ip_start+13], pkt_data[ip_start+14], pkt_data[ip_start+15]);
                        let dst_ip = format!("{}.{}.{}.{}", pkt_data[ip_start+16], pkt_data[ip_start+17], pkt_data[ip_start+18], pkt_data[ip_start+19]);
                        info["src_ip"] = serde_json::json!(src_ip);
                        info["dst_ip"] = serde_json::json!(dst_ip);
                        info["ip_proto"] = serde_json::json!(proto);
                        if (proto == 6 || proto == 17) && pkt_data.len() >= ip_start + ihl + 4 {
                            let tcp_start = ip_start + ihl;
                            if pkt_data.len() >= tcp_start + 4 {
                                let src_port = u16::from_be_bytes([pkt_data[tcp_start], pkt_data[tcp_start+1]]);
                                let dst_port = u16::from_be_bytes([pkt_data[tcp_start+2], pkt_data[tcp_start+3]]);
                                info["src_port"] = serde_json::json!(src_port);
                                info["dst_port"] = serde_json::json!(dst_port);
                            }
                        }
                    }
                }
            }
            packets.push(info);
            offset += 16 + incl_len;
            idx += 1;
        }
        let out = serde_json::json!({ "linktype": linktype, "packet_count": packets.len(), "packets": packets });
        let pretty = serde_json::to_string_pretty(&out).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&PcapParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    fn make_pcap_with_ip() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&0i32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&65535u32.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes()); // EN10MB
        // Ethernet + IPv4 + TCP
        let mut pkt = Vec::new();
        pkt.extend_from_slice(&[0x00,0x11,0x22,0x33,0x44,0x55, 0x66,0x77,0x88,0x99,0xaa,0xbb, 0x08,0x00]); // eth
        pkt.extend_from_slice(&[0x45,0x00,0x00,0x28,0x00,0x00,0x40,0x00,0x40,0x06,0x00,0x00, 192,168,1,1, 10,0,0,1]); // ip
        pkt.extend_from_slice(&[0x04,0xd2,0x00,0x50,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x50,0x02,0x00,0x00,0x00,0x00,0x00,0x00]); // tcp src 1234 dst 80
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&(pkt.len() as u32).to_le_bytes());
        v.extend_from_slice(&(pkt.len() as u32).to_le_bytes());
        v.extend_from_slice(&pkt);
        v
    }

    #[test]
    fn parses_eth_ip_tcp() {
        let ctx = NullExecutionContext;
        let pcap = make_pcap_with_ip();
        let out = PcapParse.apply(Cow::Borrowed(&pcap), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["packet_count"], 1);
        let pkt = &v["packets"][0];
        assert_eq!(pkt["src_ip"], "192.168.1.1");
        assert_eq!(pkt["dst_ip"], "10.0.0.1");
        assert_eq!(pkt["src_port"], 1234);
        assert_eq!(pkt["dst_port"], 80);
    }

    #[test]
    fn rejects_invalid_magic() {
        let ctx = NullExecutionContext;
        let err = PcapParse.apply(Cow::Borrowed(b"bad"), &serde_json::json!({}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
