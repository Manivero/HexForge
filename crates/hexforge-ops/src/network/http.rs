use hexforge_core::{ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError};
use std::borrow::Cow;
use std::collections::HashMap;

pub struct HttpParse;

impl Transform for HttpParse {
    fn id(&self) -> &'static str {
        "network.http_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "HTTP Parse"
    }
    fn category(&self) -> &'static str {
        "Network"
    }
    fn capabilities(&self) -> TransformCapabilities { TransformCapabilities { deterministic: true, streamable: false, memory_cost: MemoryCost::FullBuffer } }
    fn apply<'a>(&self, input: ByteView<'a>, _params: &serde_json::Value, _ctx: &dyn ExecutionContext) -> Result<ByteView<'a>, TransformError> {
        let s = String::from_utf8_lossy(input.as_ref());
        let parts: Vec<&str> = s.split("\r\n\r\n").collect();
        let header_part = parts[0];
        let body = if parts.len() > 1 { parts[1..].join("\r\n\r\n") } else { String::new() };
        let mut lines = header_part.lines();
        let first = lines.next().ok_or_else(|| TransformError::InvalidInput { reason: "empty HTTP message".into() })?;
        let mut headers: HashMap<String, String> = HashMap::new();
        for line in lines {
            if line.is_empty() { continue; }
            if let Some((k,v)) = line.split_once(':') {
                headers.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        let out = if first.starts_with("HTTP/") {
            // Response: HTTP/1.1 200 OK
            let mut segs = first.splitn(3, ' ');
            let version = segs.next().unwrap_or("");
            let status_code = segs.next().unwrap_or("").parse::<u16>().unwrap_or(0);
            let status_text = segs.next().unwrap_or("").to_string();
            serde_json::json!({ "type": "response", "version": version, "status_code": status_code, "status_text": status_text, "headers": headers, "body": body })
        } else {
            // Request: GET /path HTTP/1.1
            let mut segs = first.splitn(3, ' ');
            let method = segs.next().unwrap_or("");
            let path = segs.next().unwrap_or("");
            let version = segs.next().unwrap_or("");
            if method.is_empty() || path.is_empty() || version.is_empty() {
                return Err(TransformError::InvalidInput { reason: format!("invalid HTTP start line: {first}") });
            }
            serde_json::json!({ "type": "request", "method": method, "path": path, "version": version, "headers": headers, "body": body })
        };
        let pretty = serde_json::to_string_pretty(&out).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&HttpParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn parses_request() {
        let ctx = NullExecutionContext;
        let req = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\nbody";
        let out = HttpParse.apply(Cow::Borrowed(req), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["method"], "GET");
        assert_eq!(v["path"], "/index.html");
        assert_eq!(v["headers"]["Host"], "example.com");
        assert_eq!(v["body"], "body");
    }

    #[test]
    fn parses_response() {
        let ctx = NullExecutionContext;
        let resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html>";
        let out = HttpParse.apply(Cow::Borrowed(resp), &serde_json::json!({}), &ctx).unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["type"], "response");
        assert_eq!(v["status_code"], 200);
    }

    #[test]
    fn rejects_empty() {
        let ctx = NullExecutionContext;
        let err = HttpParse.apply(Cow::Borrowed(b""), &serde_json::json!({}), &ctx).unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
