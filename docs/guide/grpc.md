# gRPC (optional)
Updated: 2025-09-20
Type: How‑to
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.5" data-exp=.5 data-complex=.7 data-complicated=.5 }

The unified server exposes a minimal gRPC listener when the `grpc` Cargo feature is enabled. It mirrors the triad action queue and event stream so existing workflows can adopt gRPC without reintroducing the legacy bridge.

## Enable the listener

1. Build or run `arw-server` with the feature flag:
   ```bash
   cargo run --bin arw-server --features grpc
   ```
2. By default the listener binds to `127.0.0.1:50051`. Override with `ARW_GRPC_ADDR`, e.g. `ARW_GRPC_ADDR=0.0.0.0:50051`.
3. The HTTP surface remains available; gRPC runs alongside it.

## Surface overview

| RPC | Description |
| --- | --- |
| `Healthz` | Readiness probe returning `{ ok, version }`. |
| `SubmitAction` | Enqueue a triad action (same policy and staging checks as `POST /actions`). |
| `GetAction` | Fetch action state, output, and metadata by id. |
| `StreamEvents` | Server-stream of event envelopes with optional prefix filters and replay semantics (`after`, `replay`). |

The proto definition lives in `apps/arw-server/proto/arw.proto`; code is generated during build when the feature is enabled.

## Rust tonic example

```rust
use tonic::transport::Channel;
use arw::arw_service_client::ArwServiceClient;
use arw::{EventStreamRequest, HealthCheckRequest, SubmitActionRequest};

pub mod arw { tonic::include_proto!("arw.v1"); }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("ARW_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let mut client = ArwServiceClient::connect(addr).await?;

    let health = client.healthz(HealthCheckRequest {}).await?.into_inner();
    println!("server {} ready: {}", health.version, health.ok);

    let input = serde_json::json!({"msg": "grpc"});
    let input = serde_json::from_value(input)?;
    let action = client
        .submit_action(SubmitActionRequest {
            kind: "demo.echo".into(),
            input: Some(input),
            idem_key: String::new(),
        })
        .await?
        .into_inner();
    println!("queued action {}", action.id);

    let mut stream = client
        .stream_events(EventStreamRequest {
            prefix: vec!["actions".into()],
            replay: 0,
            after: String::new(),
        })
        .await?
        .into_inner();
    if let Some(event) = stream.message().await? {
        println!("event {}: {}", event.id, event.kind);
    }

    Ok(())
}
```

*Hint:* `prost_types::Value` implements `serde::Deserialize`, so `serde_json::from_value::<prost_types::Value>(value)` is a convenient way to pass JSON payloads.
