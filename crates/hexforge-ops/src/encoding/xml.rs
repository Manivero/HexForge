use hexforge_core::{
    ByteView, ExecutionContext, MemoryCost, Transform, TransformCapabilities, TransformError,
};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use std::borrow::Cow;
use std::io::Cursor;

pub struct XmlPretty;

impl Transform for XmlPretty {
    fn id(&self) -> &'static str {
        "encoding.xml.pretty"
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn display_name(&self) -> &'static str {
        "XML Pretty"
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
        let mut reader = Reader::from_reader(input.as_ref());
        let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(e) => {
                    writer
                        .write_event(e)
                        .map_err(|e| TransformError::InvalidInput {
                            reason: format!("XML write error: {e}"),
                        })?;
                }
                Err(e) => {
                    return Err(TransformError::InvalidInput {
                        reason: format!("not valid XML: {e}"),
                    });
                }
            }
        }
        let out = writer.into_inner().into_inner();
        if out.is_empty() {
            return Err(TransformError::InvalidInput {
                reason: "input is empty or not XML".into(),
            });
        }
        Ok(Cow::Owned(out))
    }
}

inventory::submit! { crate::TransformEntry(&XmlPretty) }

#[cfg(test)]
mod tests {
    use super::*;
    use hexforge_core::transform::NullExecutionContext;

    #[test]
    fn pretty_formats() {
        let ctx = NullExecutionContext;
        let input = b"<root><child>val</child></root>";
        let out = XmlPretty
            .apply(Cow::Borrowed(input), &serde_json::json!({}), &ctx)
            .unwrap();
        let s = String::from_utf8(out.into_owned()).unwrap();
        assert!(s.contains("\n"), "pretty should contain newline");
        assert!(s.contains("  <child>"), "should indent");
        // Roundtrip parse must succeed again
        let out2 = XmlPretty
            .apply(Cow::Borrowed(s.as_bytes()), &serde_json::json!({}), &ctx)
            .unwrap();
        assert_eq!(String::from_utf8(out2.into_owned()).unwrap(), s);
    }

    #[test]
    fn rejects_invalid_xml() {
        let ctx = NullExecutionContext;
        // Truncated tag must error (EOF inside open tag)
        let err = XmlPretty
            .apply(Cow::Borrowed(b"<root"), &serde_json::json!({}), &ctx)
            .unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput { .. }));
    }
}
