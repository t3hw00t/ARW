#[derive(Debug, Clone, Default)]
pub struct PreflightInfo {
    pub content_length: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug)]
pub enum PreflightError {
    Skip(String),
    Denied { code: String, message: String },
}
