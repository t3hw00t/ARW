# gRPC (optional)
Updated: 2025-09-15
Type: How‑to
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.5" data-exp=".5" data-complex=".7" data-complicated=".5" }

ARW exposes an optional gRPC server in `arw-svc` behind the `grpc` feature.

## Enable and run

- Build with the feature:
  - `cargo build -p arw-svc --features grpc`
- Run with gRPC enabled:
  - `ARW_GRPC=1 cargo run -p arw-svc --features grpc`
- Address (overridable):
  - Default: `[::1]:50051`
  - Override: `ARW_GRPC_ADDR=0.0.0.0:50051`

The HTTP service remains available on `ARW_PORT` (default 8090).

## Health example

Proto is in `apps/arw-svc/proto/arw.proto`. Health RPC:

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

- The gRPC server publishes a `service.health` event on each `healthz` call.
- gRPC is opt-in and not required for core HTTP functionality.
