use std::fmt;

use arw_wasi::WasiError;

#[derive(Debug)]
pub enum ToolError {
    Unsupported(String),
    Invalid(String),
    Runtime(String),
    Interrupted {
        reason: String,
        detail: Option<String>,
    },
    Denied {
        reason: String,
        dest_host: Option<String>,
        dest_port: Option<i64>,
        protocol: Option<String>,
    },
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::Unsupported(id) => write!(f, "unsupported tool: {}", id),
            ToolError::Invalid(msg) => write!(f, "invalid request: {}", msg),
            ToolError::Runtime(msg) => write!(f, "runtime error: {}", msg),
            ToolError::Interrupted { reason, detail } => {
                if let Some(detail) = detail {
                    write!(f, "interrupted: {} ({})", reason, detail)
                } else {
                    write!(f, "interrupted: {}", reason)
                }
            }
            ToolError::Denied {
                reason,
                dest_host,
                dest_port,
                protocol,
            } => {
                write!(f, "denied: {}", reason)?;
                if let Some(host) = dest_host {
                    write!(f, " host={}", host)?;
                }
                if let Some(port) = dest_port {
                    write!(f, " port={}", port)?;
                }
                if let Some(proto) = protocol {
                    write!(f, " proto={}", proto)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ToolError {}

impl From<WasiError> for ToolError {
    fn from(err: WasiError) -> Self {
        match err {
            WasiError::Unsupported(name) => ToolError::Unsupported(name),
            WasiError::Runtime(msg) => ToolError::Runtime(msg),
            WasiError::Interrupted(reason) => ToolError::Interrupted {
                reason,
                detail: None,
            },
            WasiError::Denied {
                reason,
                dest_host,
                dest_port,
                protocol,
            } => ToolError::Denied {
                reason,
                dest_host,
                dest_port,
                protocol,
            },
        }
    }
}
