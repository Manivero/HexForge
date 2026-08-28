use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use std::borrow::Cow;

pub struct YamlPretty;

impl Transform for YamlPretty {
    fn id(&self) -> &'static str {
        "encoding.yaml.pretty"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "YAML Pretty"
    }
    fn category(&self) -> &'static str {
        "Encoding"
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
        let v: serde_yaml::Value =
            serde_yaml::from_slice(input.as_ref()).map_err(|e| TransformError::InvalidInput {
                reason: format!("not valid YAML: {e}"),
            })?;
        let pretty =
            serde_yaml::to_string(&v).map_err(|e| TransformError::Internal(e.to_string()))?;
        Ok(Cow::Owned(pretty.into_bytes()))
    }
}

inventory::submit! { crate::TransformEntry(&YamlPretty) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn pretty_yaml() {
        let ctx = NullExecutionContext;
        let input = b"a: 1\nb: [2, 3]\n";
        let out = YamlPretty
            .apply(Cow::Borrowed(input), &serde_json::json!({}), &ctx)
            .unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains("a: 1"));
        assert!(s.contains("b:"));
    }

    #[test]
    fn rejects_invalid_yaml() {
        let ctx = NullExecutionContext;
        let err = YamlPretty
            .apply(Cow::Borrowed(b": : :"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
