// Example WASM plug‑in implementing the `arw:tool` ABI defined in
// `crates/arw-core/wit/tool.wit`.

wit_bindgen::generate!("../arw-core/wit");

use exports::arw::tool::ToolInfo;

struct Echo;

impl exports::arw::tool::Guest for Echo {
    fn register() -> ToolInfo {
        ToolInfo {
            id: "wasm.echo".to_string(),
            version: "1.0.0".to_string(),
            summary: "Echo input back via WASM".to_string(),
            stability: "experimental".to_string(),
        }
    }

    fn invoke(input: String) -> String {
        format!("echo: {}", input)
    }
}

export!(Echo);
