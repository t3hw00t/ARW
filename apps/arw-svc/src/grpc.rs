use crate::AppState;
use serde_json::json;

#[cfg(feature = "grpc")]
pub mod proto {
    tonic::include_proto!("arw");
}

#[cfg(feature = "grpc")]
use proto::arw_service_server::{ArwService, ArwServiceServer};
#[cfg(feature = "grpc")]
use proto::{HealthCheckRequest, HealthCheckResponse};

#[derive(Clone)]
struct GrpcSvc {
    state: AppState,
}

#[cfg(feature = "grpc")]
#[tonic::async_trait]
impl ArwService for GrpcSvc {
    async fn healthz(
        &self,
        _request: tonic::Request<HealthCheckRequest>,
    ) -> Result<tonic::Response<HealthCheckResponse>, tonic::Status> {
        self.state.bus.publish(
            crate::ext::topics::TOPIC_SERVICE_HEALTH,
            &json!({"ok": true}),
        );
        Ok(tonic::Response::new(HealthCheckResponse { ok: true }))
    }
}

#[cfg(feature = "grpc")]
pub async fn serve(state: AppState) {
    use tonic::transport::Server;
    let svc = GrpcSvc { state };
    let addr = std::env::var("ARW_GRPC_ADDR")
        .unwrap_or_else(|_| "[::1]:50051".to_string())
        .parse()
        .expect("valid ARW_GRPC_ADDR");
    if let Err(e) = Server::builder()
        .add_service(ArwServiceServer::new(svc))
        .serve(addr)
        .await
    {
        eprintln!("gRPC server error: {e}");
    }
}
