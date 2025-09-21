use super::{ToolError, Value};

pub(super) async fn capture(_input: Value) -> Result<Value, ToolError> {
    Err(ToolError::Unsupported(
        "ui.screenshot.capture requires arw-server/tool_screenshots feature".into(),
    ))
}

pub(super) async fn annotate(_input: Value) -> Result<Value, ToolError> {
    Err(ToolError::Unsupported(
        "ui.screenshot.annotate_burn requires arw-server/tool_screenshots feature".into(),
    ))
}

pub(super) async fn ocr(_input: Value) -> Result<Value, ToolError> {
    Err(ToolError::Unsupported(
        "ui.screenshot.ocr requires arw-server/tool_screenshots feature".into(),
    ))
}
