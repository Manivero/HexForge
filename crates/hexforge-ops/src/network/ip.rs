use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;
use std::net::IpAddr;

pub struct IpParse;

impl Transform for IpParse {
    fn id(&self) -> &'static str {
        "network.ip_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "IP Parse"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities { TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer } }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let s = String::from_utf8_lossy(input.as_ref());
        let s = s.trim();
        let ip: IpAddr = s.parse().map_err(|e| TransformError::InvalidInput { reason: format!("not a valid IP: {e}") })?;
        let out = match ip {
            IpAddr::V4(v4) => serde_json::json!({ "version": 4, "address": v4.to_string(), "is_private": v4.is_private(), "is_loopback": v4.is_loopback() }),
            IpAddr::V6(v6) => serde_json::json!({ "version": 6, "address": v6.to_string(), "is_loopback": v6.is_loopback() }),
        };
        let pretty = serde_json::to_string_pretty(&out).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&IpParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn ipv4_parse() {
        let ctx = NullExecutionContext;
        let out = IpParse.apply(Cow::Borrowed(b"192.168.1.1"), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["version"], 4);
        assert_eq!(v["is_private"], true);
    }

    #[test]
    fn ipv6_loopback() {
        let ctx = NullExecutionContext;
        let out = IpParse.apply(Cow::Borrowed(b"::1"), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["version"], 6);
        assert_eq!(v["is_loopback"], true);
    }

    #[test]
    fn rejects_invalid() {
        let ctx = NullExecutionContext;
        let err = IpParse.apply(Cow::Borrowed(b"not an ip"), &serde_json::json!({}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
