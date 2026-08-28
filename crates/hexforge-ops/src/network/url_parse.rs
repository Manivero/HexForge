use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;
use url::Url;

pub struct UrlParse;

impl Transform for UrlParse {
    fn id(&self) -> &'static str {
        "network.url_parse"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "URL Parse"
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
    fn apply<'a>(
        &self,
        input: ByteView<'a>,
        _params: &serde_json::Value,
        _ctx: &dyn ExecutionContext,
    ) -> Result<ByteView<'a>, TransformError> {
        let s = String::from_utf8_lossy(input.as_ref());
        let s = s.trim();
        let url = Url::parse(s).map_err(|e| TransformError::InvalidInput {
            reason: format!("not a valid URL: {e}"),
        })?;
        let out = serde_json::json!({
            "scheme": url.scheme(),
            "host": url.host_str(),
            "port": url.port(),
            "path": url.path(),
            "query": url.query(),
            "fragment": url.fragment(),
            "username": if url.username().is_empty() { serde_json::Value::Null } else { serde_json::Value::String(url.username().to_string()) },
            "password": url.password().map(|p| serde_json::Value::String(p.to_string())).unwrap_or(serde_json::Value::Null),
        });
        let pretty = serde_json::to_string_pretty(&out)
            .map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&UrlParse) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn parse_full_url() {
        let ctx = NullExecutionContext;
        let url = "https://user:pass@example.com:8080/path?q=1#frag";
        let out = UrlParse
            .apply(Cow::Borrowed(url.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(v["scheme"], "https");
        assert_eq!(v["host"], "example.com");
        assert_eq!(v["port"], 8080);
        assert_eq!(v["path"], "/path");
        assert_eq!(v["query"], "q=1");
        assert_eq!(v["fragment"], "frag");
        assert_eq!(v["username"], "user");
        assert_eq!(v["password"], "pass");
    }

    #[test]
    fn rejects_invalid_url() {
        let ctx = NullExecutionContext;
        let err = UrlParse
            .apply(Cow::Borrowed(b"not a url"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
