# gRPC (optional)
Updated: 2025-09-20
Type: How‑to
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.5" data-exp=.5 data-complex=.7 data-complicated=.5 }

ARW previously exposed an optional gRPC server via the legacy bridge. That implementation has been retired alongside `arw-svc`. This page tracks the incoming unified gRPC surface; until it lands, HTTP remains the canonical interface.

## Historical health example

Legacy proto (for reference) lived in `apps/arw-svc/proto/arw.proto` with:

```
service ArwService {
  rpc Healthz(HealthCheckRequest) returns (HealthCheckResponse);
}
```

Rust tonic example:

```rust
use tonic::transport::Channel;
use arw::arw_service_client::ArwServiceClient;
use arw::HealthCheckRequest;

pub mod arw { tonic::include_proto!("arw"); }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("ARW_GRPC_ADDR").unwrap_or("http://[::1]:50051".into());
    let mut client = ArwServiceClient::connect(addr).await?;
    let resp = client.healthz(HealthCheckRequest{}).await?;
    println!("ok: {}", resp.get_ref().ok);
    Ok(())
}
```

## Notes

- Track [Roadmap → Services & Orchestration](../ROADMAP.md#services--orchestration) for the unified gRPC rollout.
- HTTP remains the supported interface for production.

