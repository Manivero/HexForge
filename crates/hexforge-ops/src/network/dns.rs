use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct DnsParse;

impl Transform for DnsParse {
    fn id(&self) -> &'static str {
        "network.dns_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "DNS Parse"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities {
        TransformCapabilities {
            deterministic: true,
            streamable: false,
            memory_cost: MemoryCost::FullBuffer,
        }
    }
    fn params_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let data = input.as_ref();
        if data.len() < 12 {
            return Err(TransformError::InvalidInput {
                reason: "too short for DNS header (need 12)".into(),
            });
        }
        let txid = u16::from_be_bytes([data[0], data[1]]);
        let flags = u16::from_be_bytes([data[2], data[3]]);
        let qdcount = u16::from_be_bytes([data[4], data[5]]);
        let ancount = u16::from_be_bytes([data[6], data[7]]);
        let nscount = u16::from_be_bytes([data[8], data[9]]);
        let arcount = u16::from_be_bytes([data[10], data[11]]);
        let qr = (flags >> 15) & 1;
        let opcode = (flags >> 11) & 0xF;
        let rcode = flags & 0xF;
        let mut offset = 12usize;
        let mut questions = Vec::new();
        // Guard against huge qdcount causing DoS: process at most 256 questions or until truncation
        let limit = (qdcount as usize).min(256);
        for _ in 0..limit {
            if offset >= data.len() {
                break;
            }
            let (name, next) = parse_name(data, offset)?;
            if next + 4 > data.len() {
                break;
            }
            let qtype = u16::from_be_bytes([data[next], data[next + 1]]);
            let qclass = u16::from_be_bytes([data[next + 2], data[next + 3]]);
            questions.push(serde_json::json!({ "name": name, "type": qtype, "class": qclass }));
            offset = next + 4;
        }
        let out = serde_json::json!({
            "txid": txid,
            "qr": qr,
            "opcode": opcode,
            "rcode": rcode,
            "qdcount": qdcount,
            "ancount": ancount,
            "nscount": nscount,
            "arcount": arcount,
            "questions": questions,
        });
        let pretty = serde_json::to_string_pretty(&out)
            .map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

fn parse_name(data: &[u8], mut off: usize) -> Result<(String, usize), TransformError> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut next_off = 0usize;
    let mut steps = 0usize;
    let mut visited_offsets = std::collections::HashSet::new();
    while off < data.len() {
        if steps > 64 {
            return Err(TransformError::InvalidInput {
                reason: "DNS name too many jumps".into(),
            });
        }
        if !visited_offsets.insert(off) {
            return Err(TransformError::InvalidInput {
                reason: "DNS name pointer loop".into(),
            });
        }
        steps += 1;
        let len = data[off] as usize;
        if len == 0 {
            off += 1;
            if !jumped {
                next_off = off;
            }
            break;
        }
        if (len & 0xC0) == 0xC0 {
            // pointer: 11xxxxxx + next byte forms 14-bit offset
            if off + 1 >= data.len() {
                return Err(TransformError::InvalidInput {
                    reason: "truncated DNS pointer".into(),
                });
            }
            let ptr = ((len & 0x3F) << 8) | data[off + 1] as usize;
            if ptr >= data.len() {
                return Err(TransformError::InvalidInput {
                    reason: format!("DNS pointer out of bounds: {ptr} >= {}", data.len()),
                });
            }
            if !jumped {
                next_off = off + 2;
            }
            off = ptr;
            jumped = true;
            continue;
        }
        // label must not have top 2 bits set (already handled pointer), and length <=63 per RFC
        if len > 63 {
            return Err(TransformError::InvalidInput {
                reason: format!("invalid DNS label length {len} (>63)"),
            });
        }
        if off + 1 + len > data.len() {
            return Err(TransformError::InvalidInput {
                reason: "truncated DNS label".into(),
            });
        }
        let label = String::from_utf8_lossy(&data[off + 1..off + 1 + len]).to_string();
        labels.push(label);
        off += 1 + len;
        if !jumped {
            next_off = off;
        }
    }
    // Handles case where loop exited via pointer without explicit terminator: already set next_off
    // For non-jumped path, next_off already set; for jumped, next_off is after pointer
    if jumped && next_off == 0 {
        // Should not happen, but guard
        next_off = off;
    }
    // Root name (empty) -> "." for visibility, else join
    let name = if labels.is_empty() {
        ".".to_string()
    } else {
        labels.join(".")
    };
    Ok((name, next_off))
}

inventory::submit! { crate::TransformEntry(&DnsParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    fn make_query() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0x1234u16.to_be_bytes()); // txid
        v.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: qr=0, rd=1
        v.extend_from_slice(&1u16.to_be_bytes()); // qdcount
        v.extend_from_slice(&0u16.to_be_bytes()); // ancount
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        // QNAME: www.example.com
        for label in ["www", "example", "com"] {
            v.push(label.len() as u8);
            v.extend_from_slice(label.as_bytes());
        }
        v.push(0);
        v.extend_from_slice(&1u16.to_be_bytes()); // A
        v.extend_from_slice(&1u16.to_be_bytes()); // IN
        v
    }

    #[test]
    fn parses_query() {
        let ctx = NullExecutionContext;
        let data = make_query();
        let out = DnsParse
            .apply(Cow::Borrowed(&data), &serde_json::json!({}), &ctx)
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["txid"], 0x1234);
        assert_eq!(v["qdcount"], 1);
        assert_eq!(v["questions"][0]["name"], "www.example.com");
    }

    #[test]
    fn rejects_short() {
        let ctx = NullExecutionContext;
        let err = DnsParse
            .apply(Cow::Borrowed(b"short"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn pointer_compression() {
        // Craft packet with compression: second question reuses first name via pointer
        let ctx = NullExecutionContext;
        let mut v = Vec::new();
        v.extend_from_slice(&0x0001u16.to_be_bytes());
        v.extend_from_slice(&0x0100u16.to_be_bytes());
        v.extend_from_slice(&2u16.to_be_bytes()); // qdcount 2
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        // Q1: a.example.com
        for label in ["a", "example", "com"] {
            v.push(label.len() as u8);
            v.extend_from_slice(label.as_bytes());
        }
        v.push(0);
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&1u16.to_be_bytes());
        // Q2: pointer to offset 12 (start of Q1 name)
        v.push(0xC0);
        v.push(12);
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&1u16.to_be_bytes());
        let out = DnsParse
            .apply(Cow::Borrowed(&v), &serde_json::json!({}), &ctx)
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(val["questions"][0]["name"], "a.example.com");
        assert_eq!(val["questions"][1]["name"], "a.example.com");
    }

    #[test]
    fn truncated_pointer_rejected() {
        let ctx = NullExecutionContext;
        // craft minimal DNS with pointer at end without enough bytes
        let mut pkt = vec![0u8; 12];
        pkt[4] = 0;
        pkt[5] = 1; // qdcount 1
        pkt.extend_from_slice(&[0xC0]); // truncated pointer (only 1 byte)
        let err = DnsParse
            .apply(Cow::Borrowed(&pkt), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn pointer_out_of_bounds_rejected() {
        let ctx = NullExecutionContext;
        let mut pkt = vec![0u8; 12];
        pkt[4] = 0;
        pkt[5] = 1;
        pkt.extend_from_slice(&[0xC0, 0xFF]); // pointer to 0x3FF far beyond packet
        pkt.extend_from_slice(&[0, 1, 0, 1]);
        let err = DnsParse
            .apply(Cow::Borrowed(&pkt), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
        let msg = match err {
            TransformError::InvalidInput { reason } => reason,
            _ => String::new(),
        };
        assert!(msg.contains("out of bounds"));
    }

    #[test]
    fn pointer_loop_rejected() {
        let ctx = NullExecutionContext;
        // Pointer that points to itself (offset 12)
        let mut pkt = vec![0u8; 12];
        pkt[4] = 0;
        pkt[5] = 1;
        pkt.extend_from_slice(&[0xC0, 0x0C]); // ptr to 12
        pkt.extend_from_slice(&[0, 1, 0, 1]);
        // parse_name at 12 will jump to 12 infinitely; should detect loop
        let err = DnsParse
            .apply(Cow::Borrowed(&pkt), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn invalid_label_length_rejected() {
        let ctx = NullExecutionContext;
        let mut pkt = vec![0u8; 12];
        pkt[4] = 0;
        pkt[5] = 1;
        pkt.push(64); // length 64 >63 invalid
        pkt.extend_from_slice(&[b'a'; 64]);
        pkt.push(0);
        pkt.extend_from_slice(&[0, 1, 0, 1]);
        let err = DnsParse
            .apply(Cow::Borrowed(&pkt), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn truncated_label_rejected() {
        let ctx = NullExecutionContext;
        let mut pkt = vec![0u8; 12];
        pkt[4] = 0;
        pkt[5] = 1;
        pkt.push(5); // claim 5 bytes but only 2 available before truncation (plus qtype/qclass)
        pkt.extend_from_slice(b"ab");
        // not enough bytes for label+terminator+qtype/qclass, but parse_name should detect truncated label
        let err = DnsParse
            .apply(Cow::Borrowed(&pkt), &serde_json::json!({}), &ctx)
            .unwrap_err();
        // The error may be truncated label or truncated due to insufficient bytes for qtype/qclass break;
        // In current impl, parse_name returns truncated label, otherwise we break and produce 0 questions.
        // Let's craft packet that definitely triggers truncated label error by providing enough header but truncated label data
        // Actually our pkt length is 12+1+2=15, parse_name will see off+1+len=12+1+5=18 >15 -> error
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
