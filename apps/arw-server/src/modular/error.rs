use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModularValidationError {
    #[error("schema validation failed: {0:?}")]
    Schema(Vec<String>),
    #[error("invalid payload: {0}")]
    Invalid(String),
    #[error("lease {id} is not active")]
    MissingLease { id: String },
    #[error("lease {id} expired at {expired}")]
    ExpiredLease { id: String, expired: String },
    #[error("capability {capability} requires an active lease")]
    MissingCapability { capability: String },
    #[error("internal error: {0}")]
    Internal(String),
}
