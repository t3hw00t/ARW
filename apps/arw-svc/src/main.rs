#![allow(clippy::needless_return)]

pub(crate) use arw_svc::app_state::AppState;
pub use arw_svc::resources;

mod bootstrap;
mod dyn_timeout;
mod ext;
#[cfg(feature = "grpc")]
mod grpc;
mod route_recorder;
#[cfg(test)]
mod test_support;

#[tokio::main]
async fn main() {
    arw_otel::init();
    if let Err(err) = bootstrap::run().await {
        tracing::error!(?err, "arw-svc terminated with error");
    }
}
