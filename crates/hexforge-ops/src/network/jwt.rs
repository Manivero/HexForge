use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct JwtDecode;

impl Transform for JwtDecode {
    fn id(&self) -> &'static str {
        "network.jwt_decode"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "JWT Decode"
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
        let raw: Vec<&str> = s.split('.').collect();
        if raw.len() != 3 {
            return Err(TransformError::InvalidInput {
                reason: format!("JWT must have 3 dot-separated parts, got {}", raw.len()),
            });
        }
        // Толерантность к '='-паддингу: спецификация JWT его запрещает,
        // но реальные продюсеры иногда эмиттят паддингованный base64url.
        // Проверки — по обрезанным частям, в выводе signature — как в вводе.
        let parts: Vec<&str> = raw.iter().map(|p| p.trim_end_matches('=')).collect();
        let header_json = decode_b64_json(parts[0], "header")?;
        let payload_json = decode_b64_json(parts[1], "payload")?;
        // Signature stays as-is (base64url); we just verify it is valid base64url
        URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|e| TransformError::InvalidInput {
                reason: format!("invalid JWT signature base64url: {e}"),
            })?;
        let out = serde_json::json!({
            "header": header_json,
            "payload": payload_json,
            "signature": raw[2]
        });
        let pretty = serde_json::to_string_pretty(&out)
            .map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

fn decode_b64_json(part: &str, name: &str) -> Result<serde_json::Value, TransformError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|e| TransformError::InvalidInput {
            reason: format!("invalid JWT {name} base64url: {e}"),
        })?;
    serde_json::from_slice(&bytes).map_err(|e| TransformError::InvalidInput {
        reason: format!("invalid JWT {name} JSON: {e}"),
    })
}

inventory::submit! { crate::TransformEntry(&JwtDecode) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn decode_known_jwt() {
        // header {"alg":"HS256"} payload {"sub":"123","name":"Test"}
        // Generated via jwt.io with secret "secret"
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMiLCJuYW1lIjoiVGVzdCJ9.4sU7E8W3yD9zQY1p0p0p0p0p0p0p0p0p0p0p0p0p0";
        // Use a real token: header eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 payload eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ signature SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c
        let real = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let ctx = NullExecutionContext;
        let out = JwtDecode
            .apply(Cow::Borrowed(real.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        let text = String::from_utf8(out.into_owned()).unwrap();
        assert!(text.contains("\"sub\""));
        assert!(text.contains("1234567890"));
        assert!(text.contains("\"header\""));
        assert!(text.contains("\"payload\""));
        // invalid
        let _ = jwt; // suppress unused
    }

    #[test]
    fn rejects_invalid_jwt() {
        let ctx = NullExecutionContext;
        let err = JwtDecode
            .apply(
                Cow::Borrowed(b"not.a.jwt.at.all.extra"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
        let err2 = JwtDecode
            .apply(
                Cow::Borrowed(b"only.two.parts"),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap_err();
        assert!(matches!(err2, TransformError::InvalidInput { .. }));
    }

    #[test]
    fn decode_accepts_padded_base64url_parts() {
        use base64::{engine::general_purpose::URL_SAFE, Engine as _};
        // Паддингованный base64url (с '=') спецификацией JWT запрещён,
        // но встречается у реальных продюсеров — обязаны принимать.
        let h = URL_SAFE.encode(br#"{"a":1}"#);
        let p = URL_SAFE.encode(br#"{"sub":"1"}"#);
        let sig = URL_SAFE.encode(b"signature!");
        assert!(
            h.contains('=') && p.contains('=') && sig.contains('='),
            "fixture must carry '=' padding: {h} {p} {sig}"
        );
        let token = format!("{h}.{p}.{sig}");
        let ctx = NullExecutionContext;
        let out = JwtDecode
            .apply(
                Cow::Borrowed(token.as_bytes()),
                &serde_json::json!({}),
                &ctx,
            )
            .unwrap();
        let text = String::from_utf8(out.into_owned()).unwrap();
        assert!(text.contains("\"sub\""), "payload missing: {text}");
        // Echo сигнатуры — байт-в-байт как во вводе (с паддингом).
        assert!(text.contains(&sig), "signature echo changed: {text}");
    }
}
