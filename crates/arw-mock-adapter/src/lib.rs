use arw_runtime::{
    AdapterError, PrepareContext, PreparedRuntime, RuntimeAdapter, RuntimeAdapterMetadata,
    RuntimeHandle, RuntimeHealthReport, RuntimeModality, RuntimeState, RuntimeStatus,
};

pub struct MockAdapter;

#[async_trait::async_trait]
impl RuntimeAdapter for MockAdapter {
    fn id(&self) -> &'static str {
        "mock.adapter"
    }

    fn metadata(&self) -> RuntimeAdapterMetadata {
        RuntimeAdapterMetadata {
            modalities: vec![RuntimeModality::Text],
            ..Default::default()
        }
    }

    async fn prepare(&self, _ctx: PrepareContext<'_>) -> Result<PreparedRuntime, AdapterError> {
        Ok(PreparedRuntime {
            command: "mock".to_string(),
            args: vec![],
            runtime_id: Some("mock-rt".to_string()),
        })
    }

    async fn launch(&self, prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError> {
        Ok(RuntimeHandle {
            id: prepared.runtime_id.unwrap_or_else(|| "mock-rt".to_string()),
            pid: None,
        })
    }

    async fn shutdown(&self, _handle: RuntimeHandle) -> Result<(), AdapterError> {
        Ok(())
    }

    async fn health(&self, handle: &RuntimeHandle) -> Result<RuntimeHealthReport, AdapterError> {
        let status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
            .with_summary("Mock adapter ready");
        Ok(RuntimeHealthReport { status })
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_adapter() -> arw_runtime::BoxedAdapter {
    Box::new(MockAdapter)
}
