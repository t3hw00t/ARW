//! Shared data contracts spanning persona, memory, affect, economy and policy modules.
//! These mirror the JSON Schemas stored under `spec/` and provide typed validation hooks.

mod affect;
mod economy;
mod memory;
mod persona;
mod pointer;
mod policy;
mod worldview;

pub use affect::*;
pub use economy::*;
pub use memory::*;
pub use persona::*;
pub use pointer::*;
pub use policy::*;
pub use worldview::*;

/// Shared error type for contract validation routines.
#[derive(thiserror::Error, Debug)]
pub enum ContractError {
    #[error("invalid pointer: {0}")]
    InvalidPointer(String),
    #[error("assertion failed: {0}")]
    AssertionFailed(&'static str),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Helper trait for performing lightweight semantic validation on top of schema-compatible
/// deserialization. Implemented for core contracts to allow server-side guard rails prior to use
/// in the planner/executor pipeline.
pub trait Validate {
    fn validate(&self) -> Result<(), ContractError>;
}
