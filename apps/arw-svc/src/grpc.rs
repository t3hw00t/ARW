use crate::AppState;
use serde_json::json;
use tonic::{transport::Server, Request, Response, Status};

pub mod proto {
    tonic::include_proto!("arw");
}

use proto::arw_service_server::{ArwService, ArwServiceServer};
use proto::{HealthCheckRequest, HealthCheckResponse};

#[derive(Clone)]
struct GrpcSvc {
    state: AppState,
}

#[tonic::async_trait]
impl ArwService for GrpcSvc {
    async fn healthz(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        self.state
            .bus
            .publish("Service.Health", &json!({"ok": true}));
        Ok(Response::new(HealthCheckResponse { ok: true }))
    }
}

pub async fn serve(state: AppState) {
    let svc = GrpcSvc { state };
    if let Err(e) = Server::builder()
        .add_service(ArwServiceServer::new(svc))
        .serve("[::1]:50051".parse().expect("valid addr"))
        .await
    {
        eprintln!("gRPC server error: {e}");
    }
}
