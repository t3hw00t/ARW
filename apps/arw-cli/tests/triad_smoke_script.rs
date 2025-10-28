#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use assert_cmd::prelude::*;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use httpmock::prelude::*;
use predicates::prelude::*;
use serde_json::json;
use which::which;

fn tool_available(name: &str) -> bool {
    which(name).is_ok()
}

#[test]
fn triad_smoke_script_tags_persona() -> Result<()> {
    if !(tool_available("bash") && tool_available("python3") && tool_available("curl")) {
        eprintln!("Skipping triad_smoke_script_tags_persona: required tools missing");
        return Ok(());
    }

    let server = MockServer::start();
    let persona = "persona.triad";
    let action_id = "action-123";

    server.mock(|when, then| {
        when.method(GET).path("/healthz");
        then.status(200);
    });

    let actions_post = server.mock(|when, then| {
        when.method(POST)
            .path("/actions")
            .header("content-type", "application/json")
            .body_contains("\"persona_id\": \"persona.triad\"");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "id": action_id }));
    });

    let actions_get = server.mock(|when, then| {
        when.method(GET).path(format!("/actions/{action_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "state": "completed",
                "output": { "echo": "triad-smoke" },
                "persona_id": persona,
                "created": "2025-10-22T12:00:00Z"
            }));
    });

    let state_projects = server.mock(|when, then| {
        when.method(GET).path("/state/projects");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "generated": "2025-10-22T12:00:00Z", "items": [] }));
    });

    let events = server.mock(|when, then| {
        when.method(GET).path("/events").query_param("replay", "1");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body("event: service.connected\n\n");
    });

    let script_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/triad_smoke.sh");

    let mut cmd = Command::new("bash");
    cmd.arg(script_path)
        .env("TRIAD_SMOKE_BASE_URL", server.base_url())
        .env("TRIAD_SMOKE_PERSONA", persona)
        .env("TRIAD_SMOKE_TIMEOUT_SECS", "10")
        .env("ARW_TRIAD_SMOKE_ADMIN_TOKEN", "test-token");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(persona));

    actions_post.assert();
    actions_get.assert();
    state_projects.assert();
    events.assert_hits(2);

    Ok(())
}

#[test]
fn triad_smoke_script_uses_basic_auth_header() -> Result<()> {
    if !(tool_available("bash") && tool_available("python3") && tool_available("curl")) {
        eprintln!("Skipping triad_smoke_script_uses_basic_auth_header: required tools missing");
        return Ok(());
    }

    let server = MockServer::start();
    let persona = "persona.basic";
    let action_id = "action-789";
    let user = "smoke-user";
    let password = "s3cr3t";
    let expected_auth = format!("Basic {}", BASE64.encode(format!("{user}:{password}")));

    let healthz = server.mock(|when, then| {
        when.method(GET)
            .path("/healthz")
            .header("authorization", expected_auth.as_str());
        then.status(200);
    });

    let actions_post = server.mock(|when, then| {
        when.method(POST)
            .path("/actions")
            .header("content-type", "application/json")
            .body_contains("\"persona_id\": \"persona.basic\"");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "id": action_id }));
    });

    let actions_get = server.mock(|when, then| {
        when.method(GET).path(format!("/actions/{action_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "state": "completed",
                "output": { "echo": "triad-smoke" },
                "persona_id": persona,
                "created": "2025-10-22T12:10:00Z"
            }));
    });

    let state_projects = server.mock(|when, then| {
        when.method(GET).path("/state/projects");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "generated": "2025-10-22T12:10:00Z", "items": [] }));
    });

    let events = server.mock(|when, then| {
        when.method(GET).path("/events").query_param("replay", "1");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body("event: service.connected\n\n");
    });

    let script_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/triad_smoke.sh");

    let mut cmd = Command::new("bash");
    cmd.arg(script_path)
        .env("TRIAD_SMOKE_BASE_URL", server.base_url())
        .env("TRIAD_SMOKE_PERSONA", persona)
        .env("TRIAD_SMOKE_TIMEOUT_SECS", "10")
        .env("TRIAD_SMOKE_AUTH_MODE", "basic")
        .env("TRIAD_SMOKE_BASIC_USER", user)
        .env("TRIAD_SMOKE_BASIC_PASSWORD", password)
        .env("ARW_TRIAD_SMOKE_ADMIN_TOKEN", "admin-token");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(persona));

    healthz.assert();
    actions_post.assert();
    actions_get.assert();
    state_projects.assert();
    events.assert_hits(2);

    Ok(())
}

#[test]
fn triad_smoke_script_uses_healthz_bearer_header() -> Result<()> {
    if !(tool_available("bash") && tool_available("python3") && tool_available("curl")) {
        eprintln!("Skipping triad_smoke_script_uses_healthz_bearer_header: required tools missing");
        return Ok(());
    }

    let server = MockServer::start();
    let persona = "persona.health";
    let action_id = "action-456";
    let health_token = "health-secret";

    let healthz = server.mock(|when, then| {
        when.method(GET)
            .path("/healthz")
            .header("authorization", "Bearer health-secret");
        then.status(200);
    });

    let actions_post = server.mock(|when, then| {
        when.method(POST)
            .path("/actions")
            .header("content-type", "application/json")
            .body_contains("\"persona_id\": \"persona.health\"");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "id": action_id }));
    });

    let actions_get = server.mock(|when, then| {
        when.method(GET).path(format!("/actions/{action_id}"));
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "state": "completed",
                "output": { "echo": "triad-smoke" },
                "persona_id": persona,
                "created": "2025-10-22T12:05:00Z"
            }));
    });

    let state_projects = server.mock(|when, then| {
        when.method(GET).path("/state/projects");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({ "generated": "2025-10-22T12:05:00Z", "items": [] }));
    });

    let events = server.mock(|when, then| {
        when.method(GET).path("/events").query_param("replay", "1");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body("event: service.connected\n\n");
    });

    let script_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/triad_smoke.sh");

    let mut cmd = Command::new("bash");
    cmd.arg(script_path)
        .env("TRIAD_SMOKE_BASE_URL", server.base_url())
        .env("TRIAD_SMOKE_PERSONA", persona)
        .env("TRIAD_SMOKE_TIMEOUT_SECS", "10")
        .env("TRIAD_SMOKE_HEALTHZ_BEARER", health_token)
        .env("ARW_TRIAD_SMOKE_ADMIN_TOKEN", "admin-token");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(persona));

    healthz.assert();
    actions_post.assert();
    actions_get.assert();
    state_projects.assert();
    events.assert_hits(2);

    Ok(())
}
