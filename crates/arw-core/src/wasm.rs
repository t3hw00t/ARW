//! Helpers for WASM plug‑ins (experimental, feature = "wasm").

use anyhow::{anyhow, Result};
use wasmtime::component::Component;
use wasmtime::Store;
// Re-export the Engine type for downstream helpers
pub use wasmtime::Engine;

/// Minimal tool metadata placeholder (ABI pending)
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub id: String,
    pub version: String,
    pub summary: String,
    pub stability: String,
}

/// Minimal runtime wrapper. Note: ABI integration pending.
pub struct WasmTool {
    #[allow(unused)]
    component: Component,
    #[allow(dead_code)]
    store: Store<()>,
    info: ToolInfo,
}

impl WasmTool {
    /// Attempt to load a plug‑in from raw bytes. Returns an error for invalid bytes.
    pub fn from_bytes(engine: &Engine, bytes: &[u8]) -> Result<Self> {
        let component = Component::from_binary(engine, bytes)?;
        let store = Store::new(engine, ());
        Ok(Self {
            component,
            store,
            info: ToolInfo {
                id: "unknown".into(),
                version: "0.0.0".into(),
                summary: "WASM tool (placeholder)".into(),
                stability: "experimental".into(),
            },
        })
    }

    pub fn info(&self) -> &ToolInfo {
        &self.info
    }

    pub fn invoke(&mut self, _input: &str) -> Result<String> {
        Err(anyhow!(
            "invoke not implemented (experimental wasm feature)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_engine_compiles_component_or_errors() {
        let engine = Engine::default();
        // Passing garbage bytes should return an error but exercise the code path.
        let bytes = b"not-a-component";
        let res = WasmTool::from_bytes(&engine, bytes);
        assert!(res.is_err());
    }
}
