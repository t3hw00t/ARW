use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{extract::Path, extract::Query, Json};
use base64::Engine as _;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::time::SystemTime;
use tokio::fs as afs;
use tokio::io::AsyncWriteExt;

use crate::{admin_ok, read_models, AppState};
use arw_topics as topics;

fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn problem(status: axum::http::StatusCode, title: &str, detail: Option<&str>) -> Response {
    let mut body = json!({"type":"about:blank","title": title,"status": status.as_u16()});
    if let Some(d) = detail {
        body["detail"] = json!(d);
    }
    (status, Json(body)).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProjectCreateRequest {
    pub name: String,
}

pub async fn projects_create(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ProjectCreateRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let Some(safe) = sanitize_project_name(&req.name) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid project name"),
        );
    };
    let root = projects_dir();
    let dir = root.join(&safe);
    if let Err(e) = afs::create_dir_all(&dir).await {
        return problem(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Error",
            Some(&e.to_string()),
        );
    }
    let notes = dir.join("NOTES.md");
    if afs::metadata(&notes).await.is_err() {
        let ts = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let body = format!("# {}\n\nCreated: {}\n\n", safe, ts);
        let _ = save_bytes_atomic(&notes, body.as_bytes()).await;
    }
    let mut payload = json!({"name": safe.clone()});
    ensure_corr(&mut payload);
    state
        .bus()
        .publish(topics::TOPIC_PROJECTS_CREATED, &payload);
    publish_audit("projects.created", &payload).await;
    Json(json!({"name": safe})).into_response()
}

#[derive(Deserialize)]
pub struct ProjectsTreeQuery {
    pub proj: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StateProjectTreeQuery {
    #[serde(default)]
    pub path: Option<String>,
}

pub async fn projects_tree(
    headers: HeaderMap,
    Query(q): Query<ProjectsTreeQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let Some(proj_name) = q.proj.as_deref() else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("missing proj"),
        );
    };
    let Some(root) = project_root(proj_name) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };
    let rel_input = q.path.as_deref().unwrap_or("");
    let Some(rel_path) = validate_rel_path(rel_input) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid path"),
        );
    };
    let abs = root.join(&rel_path);
    let target = match afs::metadata(&abs).await {
        Ok(m) if m.is_dir() => abs.clone(),
        Ok(_) => abs.parent().unwrap_or(&root).to_path_buf(),
        Err(_) => abs.clone(),
    };
    let mut items = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&target).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = ent.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            let rel = match ent.path().strip_prefix(&root) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => name.clone(),
            };
            items.push(json!({"name": name, "dir": is_dir, "rel": rel}));
        }
    }
    items.sort_by(|a, b| {
        let ad = a.get("dir").and_then(|v| v.as_bool()).unwrap_or(false);
        let bd = b.get("dir").and_then(|v| v.as_bool()).unwrap_or(false);
        match (ad, bd) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or("")),
        }
    });
    Json(json!({"items": items})).into_response()
}

#[derive(Deserialize)]
pub struct ProjectNotesQuery {
    pub proj: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProjectNotesDocument {
    pub proj: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProjectNotesWrite {
    pub content: String,
    #[serde(default)]
    pub prev_sha256: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProjectNotesSaveResponse {
    pub ok: bool,
    pub proj: String,
    pub sha256: String,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corr_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProjectPathQuery {
    pub path: String,
}

pub async fn projects_notes_get(
    headers: HeaderMap,
    Query(q): Query<ProjectNotesQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let proj = q.proj;
    let Some(path) = project_notes_path(&proj) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };

    let meta = match afs::metadata(&path).await {
        Ok(m) => Some(m),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&err.to_string()),
            )
        }
    };

    let mut doc = ProjectNotesDocument {
        proj: proj.clone(),
        content: String::new(),
        sha256: None,
        bytes: None,
        modified: None,
    };

    if let Some(meta) = &meta {
        doc.bytes = Some(meta.len());
        doc.modified = meta.modified().ok().and_then(|t| system_time_to_rfc3339(t));
    }

    match afs::read(&path).await {
        Ok(bytes) => {
            doc.sha256 = Some(sha256_hex(&bytes));
            match String::from_utf8(bytes) {
                Ok(text) => {
                    doc.content = text;
                }
                Err(_) => {
                    return problem(
                        axum::http::StatusCode::BAD_REQUEST,
                        "Bad Request",
                        Some("notes not valid utf-8"),
                    )
                }
            }
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&err.to_string()),
            )
        }
    }

    Json(doc).into_response()
}

pub async fn projects_notes_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<ProjectNotesQuery>,
    Json(body): Json<ProjectNotesWrite>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let proj = q.proj;
    let Some(path) = project_notes_path(&proj) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };
    if let Some(expected) = body.prev_sha256.as_deref() {
        match afs::read(&path).await {
            Ok(current) => {
                let have = sha256_hex(&current);
                if have != expected {
                    return problem(
                        axum::http::StatusCode::CONFLICT,
                        "Conflict",
                        Some("sha mismatch"),
                    );
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return problem(
                    axum::http::StatusCode::CONFLICT,
                    "Conflict",
                    Some("notes missing"),
                );
            }
            Err(err) => {
                return problem(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Error",
                    Some(&err.to_string()),
                );
            }
        }
    }

    if let Some(parent) = path.parent() {
        if let Err(e) = afs::create_dir_all(parent).await {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&e.to_string()),
            );
        }
    }

    let bytes = body.content.into_bytes();
    let sha = sha256_hex(&bytes);
    if let Err(e) = save_bytes_atomic(&path, &bytes).await {
        return problem(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Error",
            Some(&e.to_string()),
        );
    }

    let meta = match afs::metadata(&path).await {
        Ok(m) => Some(m),
        Err(err) => {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&err.to_string()),
            )
        }
    };
    let (bytes_len, modified) = if let Some(meta) = meta {
        let bytes_len = meta.len();
        let modified = meta.modified().ok().and_then(|t| system_time_to_rfc3339(t));
        (bytes_len, modified)
    } else {
        (bytes.len() as u64, None)
    };

    let corr = uuid::Uuid::new_v4().to_string();
    let mut evt = json!({
        "name": proj.clone(),
        "sha256": sha,
        "bytes": bytes_len,
        "modified": modified.clone(),
        "corr_id": corr,
    });
    ensure_corr(&mut evt);
    state
        .bus()
        .publish(topics::TOPIC_PROJECTS_NOTES_SAVED, &evt);
    publish_audit("projects.notes.saved", &evt).await;

    let resp = ProjectNotesSaveResponse {
        ok: true,
        proj,
        sha256: sha,
        bytes: bytes_len,
        modified,
        corr_id: evt
            .get("corr_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    Json(resp).into_response()
}

#[derive(Deserialize)]
pub struct ProjectFileQuery {
    pub proj: String,
    pub path: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProjectFileWrite {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_b64: Option<String>,
    #[serde(default)]
    prev_sha256: Option<String>,
}

pub async fn projects_file_get(
    headers: HeaderMap,
    Query(q): Query<ProjectFileQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let Some(root) = project_root(&q.proj) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };
    let Some(rel) = validate_rel_path(&q.path) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid path"),
        );
    };
    let abs = root.join(&rel);
    let bytes = match afs::read(&abs).await {
        Ok(b) => b,
        Err(_) => {
            return problem(
                axum::http::StatusCode::NOT_FOUND,
                "Not Found",
                Some("missing file"),
            )
        }
    };
    if (bytes.len() as u64) > max_file_bytes() {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("file too large"),
        );
    }
    let sha = sha256_hex(&bytes);
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return problem(
                axum::http::StatusCode::BAD_REQUEST,
                "Bad Request",
                Some("non-utf8 file"),
            )
        }
    };
    Json(json!({
        "path": q.path,
        "sha256": sha,
        "content": content,
        "abs_path": abs.to_string_lossy(),
    }))
    .into_response()
}

pub async fn projects_file_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<ProjectFileQuery>,
    Json(body): Json<ProjectFileWrite>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let Some(root) = project_root(&q.proj) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };
    let Some(rel) = validate_rel_path(&q.path) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid path"),
        );
    };
    let abs = root.join(&rel);
    let bytes = match body.content_b64.as_deref() {
        Some(b64) => match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(b) => b,
            Err(_) => {
                return problem(
                    axum::http::StatusCode::BAD_REQUEST,
                    "Bad Request",
                    Some("invalid base64"),
                )
            }
        },
        None => match body.content.as_deref() {
            Some(text) => text.as_bytes().to_vec(),
            None => {
                return problem(
                    axum::http::StatusCode::BAD_REQUEST,
                    "Bad Request",
                    Some("missing content"),
                )
            }
        },
    };
    if (bytes.len() as u64) > max_file_bytes() {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("content too large"),
        );
    }
    if let Some(expected) = body.prev_sha256.as_deref() {
        if let Ok(prev) = afs::read(&abs).await {
            let have = sha256_hex(&prev);
            if have != expected {
                return problem(
                    axum::http::StatusCode::CONFLICT,
                    "Conflict",
                    Some("sha mismatch"),
                );
            }
        }
    }
    if let Some(parent) = abs.parent() {
        if let Err(e) = afs::create_dir_all(parent).await {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&e.to_string()),
            );
        }
    }
    if let Err(e) = save_bytes_atomic(&abs, &bytes).await {
        return problem(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Error",
            Some(&e.to_string()),
        );
    }
    let mut evt = json!({"proj": q.proj, "path": q.path});
    ensure_corr(&mut evt);
    state
        .bus()
        .publish(topics::TOPIC_PROJECTS_FILE_WRITTEN, &evt);
    publish_audit("projects.file.write", &evt).await;
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProjectPatchRequest {
    pub mode: String,
    pub content: String,
    #[serde(default)]
    pub prev_sha256: Option<String>,
}

pub async fn projects_file_patch(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<ProjectFileQuery>,
    Json(req): Json<ProjectPatchRequest>,
) -> Response {
    if req.mode.as_str() != "replace" {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("unsupported mode"),
        );
    }
    let body = ProjectFileWrite {
        content: Some(req.content),
        content_b64: None,
        prev_sha256: req.prev_sha256,
    };
    projects_file_set(headers, State(state), Query(q), Json(body))
        .await
        .into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProjectImportRequest {
    pub proj: String,
    pub dest: String,
    pub src_path: String,
    #[serde(default)]
    pub mode: Option<String>,
}

pub async fn projects_import(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ProjectImportRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let Some(root) = project_root(&req.proj) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid proj"),
        );
    };
    let shots = screenshots_dir();
    let src = std::path::Path::new(&req.src_path).to_path_buf();
    let src_can = src.canonicalize().unwrap_or(src.clone());
    let shots_can = shots.canonicalize().unwrap_or(shots.clone());
    if !src_can.starts_with(&shots_can) {
        return problem(
            axum::http::StatusCode::FORBIDDEN,
            "Forbidden",
            Some("src not under screenshots"),
        );
    }
    let Some(dest_rel) = validate_rel_path(&req.dest) else {
        return problem(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("invalid dest"),
        );
    };
    let dest_abs = root.join(&dest_rel);
    if let Some(parent) = dest_abs.parent() {
        if let Err(e) = afs::create_dir_all(parent).await {
            return problem(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&e.to_string()),
            );
        }
    }
    let mode = req.mode.as_deref().unwrap_or("copy").to_ascii_lowercase();
    let result = if mode == "move" {
        afs::rename(&src_can, &dest_abs).await
    } else {
        afs::copy(&src_can, &dest_abs).await.map(|_| ())
    };
    if let Err(e) = result {
        return problem(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Error",
            Some(&e.to_string()),
        );
    }
    let mut evt = json!({"proj": req.proj, "path": dest_rel.to_string_lossy()});
    ensure_corr(&mut evt);
    state
        .bus()
        .publish(topics::TOPIC_PROJECTS_FILE_WRITTEN, &evt);
    publish_audit("projects.file.import", &evt).await;
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    get,
    path = "/state/projects",
    tag = "State/Projects",
    responses((status = 200, description = "Projects list", body = serde_json::Value))
)]
pub async fn state_projects_list(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let snapshot = read_models::projects_snapshot().await;
    Json(snapshot).into_response()
}

#[utoipa::path(
    post,
    path = "/projects",
    tag = "Projects",
    request_body = ProjectCreateRequest,
    responses(
        (status = 200, description = "Project created", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Error")
    )
)]
pub async fn projects_create_unified(
    headers: HeaderMap,
    state: State<AppState>,
    Json(req): Json<ProjectCreateRequest>,
) -> impl IntoResponse {
    projects_create(headers, state, Json(req)).await
}

#[utoipa::path(
    get,
    path = "/state/projects/{proj}/tree",
    tag = "State/Projects",
    params(
        ("proj" = String, Path, description = "Project name"),
        ("path" = Option<String>, Query, description = "Relative path")
    ),
    responses(
        (status = 200, description = "Project tree", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found")
    )
)]
pub async fn state_projects_tree(
    headers: HeaderMap,
    Path(proj): Path<String>,
    Query(q): Query<StateProjectTreeQuery>,
) -> impl IntoResponse {
    projects_tree(
        headers,
        Query(ProjectsTreeQuery {
            proj: Some(proj),
            path: q.path,
        }),
    )
    .await
}

#[utoipa::path(
    get,
    path = "/state/projects/{proj}/notes",
    tag = "State/Projects",
    params(("proj" = String, Path, description = "Project name")),
    responses(
        (status = 200, description = "Project notes", body = ProjectNotesDocument),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn state_projects_notes(
    headers: HeaderMap,
    Path(proj): Path<String>,
) -> impl IntoResponse {
    projects_notes_get(headers, Query(ProjectNotesQuery { proj })).await
}

#[utoipa::path(
    put,
    path = "/projects/{proj}/notes",
    tag = "Projects",
    params(("proj" = String, Path, description = "Project name")),
    request_body = ProjectNotesWrite,
    responses(
        (status = 200, description = "Notes saved", body = ProjectNotesSaveResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Error")
    )
)]
pub async fn projects_notes_put(
    headers: HeaderMap,
    state: State<AppState>,
    Path(proj): Path<String>,
    Json(body): Json<ProjectNotesWrite>,
) -> impl IntoResponse {
    projects_notes_set(
        headers,
        state,
        Query(ProjectNotesQuery { proj }),
        Json(body),
    )
    .await
}

#[utoipa::path(
    get,
    path = "/state/projects/{proj}/file",
    tag = "State/Projects",
    params(
        ("proj" = String, Path, description = "Project name"),
        ("path" = String, Query, description = "Relative path")
    ),
    responses(
        (status = 200, description = "Project file", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found")
    )
)]
pub async fn state_projects_file_get(
    headers: HeaderMap,
    Path(proj): Path<String>,
    Query(q): Query<ProjectPathQuery>,
) -> impl IntoResponse {
    projects_file_get(headers, Query(ProjectFileQuery { proj, path: q.path })).await
}

#[utoipa::path(
    put,
    path = "/projects/{proj}/file",
    tag = "Projects",
    params(
        ("proj" = String, Path, description = "Project name"),
        ("path" = String, Query, description = "Relative path")
    ),
    request_body = ProjectFileWrite,
    responses(
        (status = 200, description = "File written", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Error")
    )
)]
pub async fn projects_file_put(
    headers: HeaderMap,
    state: State<AppState>,
    Path(proj): Path<String>,
    Query(q): Query<ProjectPathQuery>,
    Json(body): Json<ProjectFileWrite>,
) -> impl IntoResponse {
    projects_file_set(
        headers,
        state,
        Query(ProjectFileQuery { proj, path: q.path }),
        Json(body),
    )
    .await
}

#[utoipa::path(
    patch,
    path = "/projects/{proj}/file",
    tag = "Projects",
    params(
        ("proj" = String, Path, description = "Project name"),
        ("path" = String, Query, description = "Relative path")
    ),
    request_body = ProjectPatchRequest,
    responses(
        (status = 200, description = "File patched", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Error")
    )
)]
pub async fn projects_file_patch_unified(
    headers: HeaderMap,
    state: State<AppState>,
    Path(proj): Path<String>,
    Query(q): Query<ProjectPathQuery>,
    Json(req): Json<ProjectPatchRequest>,
) -> impl IntoResponse {
    projects_file_patch(
        headers,
        state,
        Query(ProjectFileQuery { proj, path: q.path }),
        Json(req),
    )
    .await
}

#[utoipa::path(
    post,
    path = "/projects/{proj}/import",
    tag = "Projects",
    params(("proj" = String, Path, description = "Project name")),
    request_body = ProjectImportRequest,
    responses(
        (status = 200, description = "Imported", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Error")
    )
)]
pub async fn projects_import_unified(
    headers: HeaderMap,
    state: State<AppState>,
    Path(proj): Path<String>,
    Json(req): Json<ProjectImportRequest>,
) -> impl IntoResponse {
    projects_import(headers, state, Json(ProjectImportRequest { proj, ..req })).await
}

fn projects_dir() -> std::path::PathBuf {
    if let Ok(env) = std::env::var("ARW_PROJECTS_DIR") {
        if !env.trim().is_empty() {
            return std::path::PathBuf::from(env);
        }
    }
    crate::util::state_dir().join("projects")
}

fn screenshots_dir() -> std::path::PathBuf {
    crate::util::state_dir().join("screenshots")
}

fn sanitize_project_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return None;
    }
    if trimmed.starts_with('.') {
        return None;
    }
    let ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    if !ok {
        return None;
    }
    Some(trimmed.to_string())
}

fn project_root(name: &str) -> Option<std::path::PathBuf> {
    sanitize_project_name(name).map(|s| projects_dir().join(s))
}

fn project_notes_path(name: &str) -> Option<std::path::PathBuf> {
    project_root(name).map(|p| p.join("NOTES.md"))
}

fn system_time_to_rfc3339(time: SystemTime) -> Option<String> {
    Some(DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn validate_rel_path(rel: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(rel);
    if path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        ) || {
            #[cfg(windows)]
            {
                matches!(c, std::path::Component::Prefix(_))
            }
            #[cfg(not(windows))]
            {
                false
            }
        }
    }) {
        return None;
    }
    Some(path.to_path_buf())
}

fn max_file_bytes() -> u64 {
    std::env::var("ARW_PROJECT_MAX_FILE_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1)
        .saturating_mul(1024 * 1024)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn ensure_corr(value: &mut Value) {
    if let Value::Object(map) = value {
        if !map.contains_key("corr_id") {
            map.insert(
                "corr_id".into(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
        }
    }
}

async fn save_bytes_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        afs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    afs::write(&tmp, bytes).await?;
    match afs::rename(&tmp, path).await {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = afs::remove_file(path).await;
            let result = afs::rename(&tmp, path).await;
            if result.is_err() {
                let _ = afs::remove_file(&tmp).await;
            }
            result
        }
    }
}

async fn publish_audit(action: &str, details: &Value) {
    let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let line = serde_json::json!({"time": ts, "action": action, "details": details});
    let entry = serde_json::to_string(&line).unwrap_or_else(|_| "{}".to_string()) + "\n";
    let path = crate::util::state_dir().join("audit.log");
    if let Some(parent) = path.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    if let Ok(mut f) = afs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        let _ = f.write_all(entry.as_bytes()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{HeaderMap, HeaderValue, StatusCode},
    };
    use serde_json::Value;
    use tempfile::tempdir;

    #[tokio::test]
    async fn state_projects_snapshot_includes_notes_and_tree() {
        let temp = tempdir().expect("tempdir");
        let ctx = crate::test_support::begin_state_env(temp.path());
        let state_dir = temp.path().display().to_string();

        let projects_root = temp.path().join("projects");
        std::fs::create_dir_all(projects_root.join("alpha/docs")).expect("create project dir");
        std::fs::write(projects_root.join("alpha/NOTES.md"), "Hello world").expect("write notes");
        std::fs::write(projects_root.join("alpha/docs/info.txt"), "data").expect("write file");

        let snapshot = crate::read_models::projects_snapshot_at(&projects_root).await;
        let items = snapshot["items"].as_array().expect("items array");
        let proj = items
            .iter()
            .find(|p| p["name"].as_str() == Some("alpha"))
            .expect("project entry");
        let notes = proj["notes"].as_object().expect("notes object");
        assert!(notes["content"].as_str().unwrap_or("").contains("Hello"));
        let tree = proj["tree"].as_object().expect("tree object");
        let paths = tree["paths"].as_object().expect("paths object");
        assert!(paths.contains_key(""));

        let mut env_guard = ctx.env;
        let projects_dir = projects_root.display().to_string();
        env_guard.set("ARW_STATE_DIR", &state_dir);
        env_guard.set("ARW_PROJECTS_DIR", &projects_dir);
        env_guard.set("ARW_ADMIN_TOKEN", "secret");

        let mut headers = HeaderMap::new();
        headers.insert("X-ARW-Admin", HeaderValue::from_static("secret"));

        let response = state_projects_list(headers).await.into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert!(value["items"].as_array().is_some());
    }

    #[tokio::test]
    async fn state_projects_tree_returns_entries() {
        let temp = tempdir().expect("tempdir");
        let ctx = crate::test_support::begin_state_env(temp.path());
        let state_dir = temp.path().display().to_string();

        let projects_root = temp.path().join("projects");
        std::fs::create_dir_all(projects_root.join("alpha/docs")).expect("create project dir");
        std::fs::write(projects_root.join("alpha/docs/info.txt"), "data").expect("write file");

        let mut env_guard = ctx.env;
        let projects_dir = projects_root.display().to_string();
        env_guard.set("ARW_STATE_DIR", &state_dir);
        env_guard.set("ARW_PROJECTS_DIR", &projects_dir);
        env_guard.set("ARW_ADMIN_TOKEN", "secret");

        let mut headers = HeaderMap::new();
        headers.insert("X-ARW-Admin", HeaderValue::from_static("secret"));

        // Root listing
        let response = state_projects_tree(
            headers.clone(),
            Path("alpha".to_string()),
            Query(StateProjectTreeQuery { path: None }),
        )
        .await
        .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().expect("items array");
        assert!(items.iter().any(|it| it["name"].as_str() == Some("docs")));

        // Nested path listing
        let response = state_projects_tree(
            headers,
            Path("alpha".to_string()),
            Query(StateProjectTreeQuery {
                path: Some("docs".to_string()),
            }),
        )
        .await
        .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes nested");
        let value: Value = serde_json::from_slice(&bytes).expect("json nested");
        let items = value["items"].as_array().expect("items array nested");
        assert!(items
            .iter()
            .any(|it| it["name"].as_str() == Some("info.txt")));
    }

    #[test]
    fn validate_rel_path_blocks_traversal_and_prefixes() {
        assert!(validate_rel_path("docs/readme.md").is_some());
        assert!(validate_rel_path("../etc/passwd").is_none());

        #[cfg(windows)]
        {
            assert!(validate_rel_path("..\\foo").is_none());
            assert!(validate_rel_path("C:secret.txt").is_none());
            assert!(validate_rel_path(r"\\server\share\data.txt").is_none());
        }
    }
}
