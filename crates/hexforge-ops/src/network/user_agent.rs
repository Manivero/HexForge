use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;

pub struct UserAgentParse;

impl Transform for UserAgentParse {
    fn id(&self) -> &'static str {
        "network.user_agent_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "User-Agent Parse"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities { TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer } }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let ua = String::from_utf8_lossy(input.as_ref());
        let ua_lower = ua.to_lowercase();
        let browser = if ua_lower.contains("edg/") || ua_lower.contains("edge") { "Edge" }
        else if ua_lower.contains("opr/") || ua_lower.contains("opera") { "Opera" }
        else if ua_lower.contains("chrome") && !ua_lower.contains("chromium") { "Chrome" }
        else if ua_lower.contains("firefox") { "Firefox" }
        else if ua_lower.contains("safari") && !ua_lower.contains("chrome") { "Safari" }
        else if ua_lower.contains("msie") || ua_lower.contains("trident") { "IE" }
        else { "Unknown" };
        let os = if ua_lower.contains("iphone") || ua_lower.contains("ipad") { "iOS" }
        else if ua_lower.contains("windows") { "Windows" }
        else if ua_lower.contains("android") { "Android" }
        else if ua_lower.contains("mac os") || ua_lower.contains("macintosh") { "macOS" }
        else if ua_lower.contains("linux") { "Linux" }
        else { "Unknown" };
        let device = if ua_lower.contains("mobile") || ua_lower.contains("iphone") || ua_lower.contains("android") { "Mobile" } else { "Desktop" };
        let out = serde_json::json!({ "browser": browser, "os": os, "device": device, "raw": ua });
        let pretty = serde_json::to_string_pretty(&out).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&UserAgentParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn chrome_windows() {
        let ctx = NullExecutionContext;
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let out = UserAgentParse.apply(Cow::Borrowed(ua.as_bytes()), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["browser"], "Chrome");
        assert_eq!(v["os"], "Windows");
        assert_eq!(v["device"], "Desktop");
    }

    #[test]
    fn iphone_safari() {
        let ctx = NullExecutionContext;
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        let out = UserAgentParse.apply(Cow::Borrowed(ua.as_bytes()), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["browser"], "Safari");
        assert_eq!(v["os"], "iOS");
        assert_eq!(v["device"], "Mobile");
    }
}
