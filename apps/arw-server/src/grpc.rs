use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::{stream::Stream, StreamExt};
use prost_types::{value::Kind, ListValue, Struct, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, warn};

use crate::api::actions::{self, ActionReq, SubmitActionError};
use crate::staging;
use crate::AppState;

use arw_kernel::{ActionRow, EventRow};

pub(crate) mod proto {
    tonic::include_proto!("arw.v1");
}

use proto::arw_service_server::{ArwService, ArwServiceServer};
use proto::{
    Action as GrpcAction, EventEnvelope, EventStreamRequest, GetActionRequest, HealthCheckRequest,
    HealthCheckResponse, SubmitActionRequest, SubmitActionResponse,
};

pub(crate) fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(err) = serve(state).await {
            tracing::error!(target: "grpc", "gRPC server exited: {err}");
        }
    })
}

async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = parse_addr();
    info!(target: "grpc", %addr, "starting gRPC listener");
    Server::builder()
        .add_service(ArwServiceServer::new(ArwGrpcService { state }))
        .serve(addr)
        .await?;
    Ok(())
}

fn parse_addr() -> SocketAddr {
    let raw = std::env::var("ARW_GRPC_ADDR").unwrap_or_else(|_| "127.0.0.1:50051".into());
    let cleaned = raw
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    cleaned
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:50051".parse().expect("default gRPC addr"))
}

struct ArwGrpcService {
    state: AppState,
}

type EventStream = Pin<Box<dyn Stream<Item = Result<EventEnvelope, Status>> + Send>>;

#[tonic::async_trait]
impl ArwService for ArwGrpcService {
    type StreamEventsStream = EventStream;

    async fn healthz(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            ok: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }))
    }

    async fn submit_action(
        &self,
        request: Request<SubmitActionRequest>,
    ) -> Result<Response<SubmitActionResponse>, Status> {
        let payload = request.into_inner();
        let input_json = payload
            .input
            .as_ref()
            .map(prost_to_json)
            .unwrap_or(serde_json::Value::Null);
        let idem_key = if payload.idem_key.is_empty() {
            None
        } else {
            Some(payload.idem_key)
        };
        let action_req = ActionReq {
            kind: payload.kind,
            input: input_json,
            idem_key,
        };
        match api::actions::submit_action(&self.state, action_req).await {
            Ok(outcome) => Ok(Response::new(SubmitActionResponse {
                id: outcome.id,
                staged: outcome.staged,
                mode: outcome
                    .stage_mode
                    .unwrap_or_else(|| staging::mode_label().to_string()),
                reused: outcome.reused,
            })),
            Err(SubmitActionError::KernelDisabled) => {
                Err(Status::failed_precondition("kernel disabled"))
            }
            Err(SubmitActionError::PolicyDenied {
                require_capability,
                explain,
            }) => {
                let mut detail = String::from("policy denied");
                if let Some(cap) = require_capability {
                    detail.push_str(&format!(": lease required for {cap}"));
                }
                detail.push_str(&format!(" ({})", explain));
                Err(Status::permission_denied(detail))
            }
            Err(SubmitActionError::QueueFull { limit, queued }) => Err(Status::resource_exhausted(
                format!("queue is full (limit={limit}, queued={queued})"),
            )),
            Err(SubmitActionError::Internal(err)) => {
                Err(Status::internal(format!("internal error: {err}")))
            }
        }
    }

    async fn get_action(
        &self,
        request: Request<GetActionRequest>,
    ) -> Result<Response<GrpcAction>, Status> {
        if !self.state.kernel_enabled() {
            return Err(Status::failed_precondition("kernel disabled"));
        }
        let req = request.into_inner();
        match self.state.kernel().get_action_async(&req.id).await {
            Ok(Some(row)) => Ok(Response::new(action_row_to_proto(row)?)),
            Ok(None) => Err(Status::not_found("action not found")),
            Err(err) => Err(Status::internal(format!("lookup failed: {err}"))),
        }
    }

    async fn stream_events(
        &self,
        request: Request<EventStreamRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let req = request.into_inner();
        let EventStreamRequest {
            prefix,
            replay,
            after,
        } = req;
        if !self.state.kernel_enabled() && (replay > 0 || !after.is_empty()) {
            return Err(Status::failed_precondition(
                "event replay unavailable when kernel disabled",
            ));
        }
        let prefixes = prefix;
        let (tx, rx) = mpsc::channel::<Result<EventEnvelope, Status>>(256);

        // Initial replay (after/resume has precedence)
        if let Some(after_val) = if after.is_empty() {
            None
        } else {
            Some(after.clone())
        } {
            if let Ok(after_id) = after_val.parse::<i64>() {
                if let Ok(rows) = self
                    .state
                    .kernel()
                    .recent_events_async(1000, Some(after_id))
                    .await
                {
                    for row in rows {
                        if tx.send(event_row_to_proto(row)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        } else if replay > 0 {
            if let Ok(rows) = self
                .state
                .kernel()
                .recent_events_async(replay as i64, None)
                .await
            {
                for row in rows {
                    if tx.send(event_row_to_proto(row)).await.is_err() {
                        break;
                    }
                }
            }
        }

        let state = self.state.clone();
        tokio::spawn(async move {
            let mut rx_bus = if prefixes.is_empty() {
                state.bus().subscribe()
            } else {
                state.bus().subscribe_filtered(prefixes, Some(256))
            };
            let cache = state.sse_cache();
            while let Ok(env) = rx_bus.recv().await {
                match envelope_to_proto(&env, &cache).await {
                    Ok(msg) => {
                        if tx.send(Ok(msg)).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        warn!(target: "grpc", "skip event: {err}");
                    }
                }
            }
        });

        let stream = ReceiverStream::new(rx).map(|res| res);
        Ok(Response::new(Box::pin(stream) as EventStream))
    }
}

fn action_row_to_proto(row: ActionRow) -> Result<GrpcAction, Status> {
    Ok(GrpcAction {
        id: row.id,
        kind: row.kind,
        state: row.state,
        input: Some(json_to_prost(&row.input)?),
        output: row.output.as_ref().map(json_to_prost).transpose()?,
        error: row.error.unwrap_or_default(),
        created: row.created,
        updated: row.updated,
        policy_ctx: row.policy_ctx.as_ref().map(json_to_prost).transpose()?,
        idem_key: row.idem_key.unwrap_or_default(),
    })
}

fn event_row_to_proto(row: EventRow) -> Result<EventEnvelope, Status> {
    Ok(EventEnvelope {
        time: row.time,
        kind: row.kind,
        payload: Some(json_to_prost(&row.payload)?),
        id: row.id.to_string(),
        policy: Some(null_value()),
    })
}

async fn envelope_to_proto(
    env: &arw_events::Envelope,
    cache: &Arc<tokio::sync::Mutex<crate::sse_cache::SseIdCache>>,
) -> Result<EventEnvelope, Status> {
    let payload = Some(json_to_prost(&env.payload)?);
    let policy_json = env
        .policy()
        .as_ref()
        .map(|p| serde_json::to_value(p).map_err(|e| Status::internal(e.to_string())));
    let policy = match policy_json {
        Some(Ok(v)) => Some(json_to_prost(&v)?),
        Some(Err(err)) => return Err(err),
        None => Some(null_value()),
    };
    let id = compute_event_id(env, cache).await;
    Ok(EventEnvelope {
        time: env.time.clone(),
        kind: env.kind.clone(),
        payload,
        id,
        policy,
    })
}

async fn compute_event_id(
    env: &arw_events::Envelope,
    cache: &Arc<tokio::sync::Mutex<crate::sse_cache::SseIdCache>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(env.time.as_bytes());
    hasher.update(env.kind.as_bytes());
    if let Ok(bytes) = serde_json::to_vec(&env.payload) {
        hasher.update(&bytes);
    }
    let digest = hasher.finalize();
    let key = u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    let cached = {
        let cache = cache.lock().await;
        cache.get(key).map(|v| v.to_string())
    };
    cached.unwrap_or_else(|| hex::encode(digest))
}

fn json_to_prost(value: &serde_json::Value) -> Result<Value, Status> {
    Ok(match value {
        serde_json::Value::Null => null_value(),
        serde_json::Value::Bool(b) => Value {
            kind: Some(Kind::BoolValue(*b)),
        },
        serde_json::Value::Number(num) => Value {
            kind: Some(Kind::NumberValue(
                num.as_f64()
                    .ok_or_else(|| Status::invalid_argument("invalid number"))?,
            )),
        },
        serde_json::Value::String(s) => Value {
            kind: Some(Kind::StringValue(s.clone())),
        },
        serde_json::Value::Array(items) => Value {
            kind: Some(Kind::ListValue(ListValue {
                values: items
                    .iter()
                    .map(json_to_prost)
                    .collect::<Result<Vec<_>, _>>()?,
            })),
        },
        serde_json::Value::Object(map) => Value {
            kind: Some(Kind::StructValue(Struct {
                fields: map
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), json_to_prost(v)?)))
                    .collect::<Result<_, Status>>()?,
            })),
        },
    })
}

fn prost_to_json(value: &Value) -> serde_json::Value {
    match value.kind.as_ref() {
        Some(Kind::NullValue(_)) | None => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::StructValue(st)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &st.fields {
                map.insert(k.clone(), prost_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Some(Kind::ListValue(list)) => {
            serde_json::Value::Array(list.values.iter().map(prost_to_json).collect())
        }
    }
}

fn null_value() -> Value {
    Value {
        kind: Some(Kind::NullValue(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capsule_guard, cluster, experiments, feedback, governor, metrics, models,
        test_support::env, tool_cache, worker,
    };
    use arw_events::Bus;
    use arw_policy::PolicyEngine;
    use arw_wasi::ToolHost;
    use futures_util::StreamExt;
    use serde_json::json;
    use std::{path::Path, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::time::{sleep, timeout};

    async fn build_state(dir: &Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(tokio::sync::Mutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        let models_store = Arc::new(models::ModelStore::new(bus.clone(), Some(kernel.clone())));
        models_store.bootstrap().await;
        let tool_cache = Arc::new(tool_cache::ToolCache::new());
        let governor_state = governor::GovernorState::new().await;
        let metrics = Arc::new(metrics::Metrics::default());
        let cluster_state = cluster::ClusterRegistry::new(bus.clone());
        let feedback_hub =
            feedback::FeedbackHub::new(bus.clone(), metrics.clone(), governor_state.clone()).await;
        let experiments_state =
            experiments::Experiments::new(bus.clone(), governor_state.clone()).await;
        let capsules_store = Arc::new(capsule_guard::CapsuleStore::new());
        AppState {
            bus,
            kernel,
            policy: policy_arc,
            host,
            config_state: Arc::new(tokio::sync::Mutex::new(json!({}))),
            config_history: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            sse_id_map: Arc::new(tokio::sync::Mutex::new(
                crate::sse_cache::SseIdCache::with_capacity(64),
            )),
            endpoints: Arc::new(Vec::new()),
            endpoints_meta: Arc::new(Vec::new()),
            metrics,
            kernel_enabled: true,
            models: models_store,
            tool_cache,
            governor: governor_state,
            feedback: feedback_hub,
            cluster: cluster_state,
            experiments: experiments_state,
            capsules: capsules_store,
        }
    }

    #[tokio::test]
    async fn health_and_action_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let mut env_guard = env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;
        let _worker = worker::start_local_worker(state.clone());
        let service = ArwGrpcService {
            state: state.clone(),
        };

        let health = service
            .healthz(Request::new(HealthCheckRequest {}))
            .await
            .expect("health response")
            .into_inner();
        assert!(health.ok);

        let submit_req = SubmitActionRequest {
            kind: "demo.echo".into(),
            input: Some(json_to_prost(&json!({"msg": "grpc"})).expect("json to prost")),
            idem_key: String::new(),
        };
        let submit_resp = service
            .submit_action(Request::new(submit_req))
            .await
            .expect("submit response")
            .into_inner();
        assert!(!submit_resp.id.is_empty());

        let mut done = false;
        for _ in 0..40 {
            let action = service
                .get_action(Request::new(GetActionRequest {
                    id: submit_resp.id.clone(),
                }))
                .await
                .expect("get action")
                .into_inner();
            if action.state == "completed" {
                let output = action.output.expect("action output");
                let json_out = prost_to_json(&output);
                assert_eq!(
                    json_out["echo"]["msg"],
                    serde_json::Value::String("grpc".into())
                );
                done = true;
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        assert!(done, "action did not complete in time");
    }

    #[tokio::test]
    async fn stream_receives_submitted_event() {
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;
        let service = ArwGrpcService {
            state: state.clone(),
        };

        let response = service
            .stream_events(Request::new(EventStreamRequest {
                prefix: vec!["actions".into()],
                replay: 0,
                after: String::new(),
            }))
            .await
            .expect("stream response");
        let mut stream = response.into_inner();

        let submit_req = SubmitActionRequest {
            kind: "demo.echo".into(),
            input: Some(json_to_prost(&json!({"msg": "event"})).expect("json to prost")),
            idem_key: String::new(),
        };
        let _ = service
            .submit_action(Request::new(submit_req))
            .await
            .expect("submit event");

        let event = timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("stream wait")
            .expect("stream item")
            .expect("event ok");
        assert_eq!(event.kind, "actions.submitted");
    }
}
