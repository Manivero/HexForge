//! Minimal HexForge WIT component plugin: uppercases ASCII input.
//!
//! Implements the `hexforge:plugin@0.1.0` / `hexforge-plugin` world from
//! `wit/plugin.wit`. The host calls `apply` with fuel + memory limits;
//! any trap is isolated and reported as a plugin error, never a host crash.

wit_bindgen::generate!({
    world: "hexforge-plugin",
    path: "wit",
});

use exports::hexforge::plugin::transform::{Capabilities, Guest};

struct Component;

impl Guest for Component {
    fn get_id() -> String {
        "example.wit-uppercase".into()
    }

    fn get_version() -> String {
        "1.0.0".into()
    }

    fn get_display_name() -> String {
        "Example WIT Uppercase".into()
    }

    fn get_category() -> String {
        "Text".into()
    }

    fn get_params_schema() -> String {
        r#"{"type":"object","properties":{}}"#.into()
    }

    fn get_capabilities() -> Capabilities {
        Capabilities {
            deterministic: true,
            streamable: false,
            memory_cost: "full-buffer".into(),
        }
    }

    fn apply(input: Vec<u8>, _params: String) -> Result<Vec<u8>, String> {
        Ok(input
            .into_iter()
            .map(|b| if b.is_ascii_lowercase() { b - 32 } else { b })
            .collect())
    }
}

export!(Component);
