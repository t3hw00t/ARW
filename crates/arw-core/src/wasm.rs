//! Helpers for loading WASM plug‑ins implementing the `tool` ABI.
#![cfg(feature = "wasm")]

use anyhow::Result;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Engine, Store};
// Re-export the Engine type for downstream macros/helpers
pub use wasmtime::Engine;

// Generate bindings from the simple tool ABI defined in `wit/tool.wit`.
bindgen!({ path: "./wit", world: "plugin" });

/// Runtime wrapper around a compiled WASM plug‑in implementing the `tool` interface.
pub struct WasmTool {
    instance: Plugin,
    store: Store<()>,
    info: ToolInfo,
}

impl WasmTool {
    /// Load a new plug‑in from raw bytes.
    pub fn from_bytes(engine: &Engine, bytes: &[u8]) -> Result<Self> {
        let component = Component::from_binary(engine, bytes)?;
        let mut linker = Linker::new(engine);
        let mut store = Store::new(engine, ());
        let instance = Plugin::new(&mut store, &component, &linker)?;
        let info = instance.call_register(&mut store)?;
        Ok(Self {
            instance,
            store,
            info,
        })
    }

    /// Metadata exposed by the plug‑in.
    pub fn info(&self) -> &ToolInfo {
        &self.info
    }

    /// Invoke the plug‑in with a JSON string.
    pub fn invoke(&mut self, input: &str) -> Result<String> {
        self.instance.call_invoke(&mut self.store, input)
    }
}
