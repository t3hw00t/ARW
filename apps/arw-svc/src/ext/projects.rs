use super::{corr, io, paths, ApiError};
use crate::AppState;
use arw_macros::arw_admin;
use axum::{extract::Query, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use tokio::fs as afs;

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ProjCreateReq {
    pub name: String,
}

#[arw_admin(method="GET", path="/admin/projects/list", summary="List projects")]
pub(crate) async fn projects_list() -> impl IntoResponse {
    let mut out: Vec<String> = Vec::new();
    let root = paths::projects_dir();
    if let Ok(mut rd) = afs::read_dir(&root).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Ok(mt) = ent.file_type().await {
                if mt.is_dir() {
                    if let Some(s) = ent.file_name().to_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }
        out.sort();
    }
    super::ok(json!({"items": out}))
}

#[arw_admin(method="POST", path="/admin/projects/create", summary="Create project")]
pub(crate) async fn projects_create(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<ProjCreateReq>,
) -> impl IntoResponse {
    let Some(safe) = paths::sanitize_project_name(&req.name) else {
        return ApiError::bad_request("invalid project name").into_response();
    };
    let root = paths::projects_dir();
    let dir = root.join(&safe);
    if let Err(e) = afs::create_dir_all(&dir).await {
        return ApiError::internal(&e.to_string()).into_response();
    }
    // Create a default NOTES.md if missing
    let notes = dir.join("NOTES.md");
    if afs::metadata(&notes).await.is_err() {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let body = format!("# {}\n\nCreated: {}\n\n", safe, ts);
        let _ = io::save_bytes_atomic(&notes, body.as_bytes()).await;
    }
    // Emit event for orchestration/agents to react to project lifecycle
    let mut p = json!({"name": safe.clone()});
    corr::ensure_corr(&mut p);
    state.bus.publish("Projects.Created", &p);
    super::ok(json!({"name": safe})).into_response()
}

#[derive(Deserialize)]
pub(crate) struct TreeQs {
    pub proj: Option<String>,
    pub path: Option<String>,
}
#[arw_admin(method="GET", path="/admin/projects/tree", summary="Project tree listing")]
pub(crate) async fn projects_tree(Query(q): Query<TreeQs>) -> impl IntoResponse {
    let Some(proj) = q.proj.as_deref() else {
        return ApiError::bad_request("missing proj").into_response();
    };
    let Some(root) = paths::project_root(proj) else {
        return ApiError::bad_request("invalid proj").into_response();
    };
    let rel = q.path.unwrap_or_default();
    // Only allow ascii safe rel components and no leading dots
    let rel_path = std::path::Path::new(&rel);
    if rel_path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return ApiError::bad_request("invalid path").into_response();
    }
    let abs = root.join(rel_path);
    // Ensure path exists and is a directory; if file, list parent
    let target = match afs::metadata(&abs).await {
        Ok(m) if m.is_dir() => abs.clone(),
        Ok(_) => abs.parent().unwrap_or(&root).to_path_buf(),
        Err(_) => abs.clone(),
    };
    let mut items = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&target).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent
                .file_name()
                .to_str()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue; // hide dotfiles
            }
            let ft = ent.file_type().await.ok();
            let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
            let rel_next = if let Ok(p) = ent.path().strip_prefix(&root) {
                p.to_string_lossy().replace('\\', "/")
            } else {
                name.clone()
            };
            items.push(json!({"name": name, "dir": is_dir, "rel": rel_next}));
        }
        // Folders first, then files; alpha within groups
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
    }
    super::ok(json!({"items": items})).into_response()
}

#[derive(Deserialize)]
pub(crate) struct NotesQs {
    pub proj: String,
}
#[arw_admin(method="GET", path="/admin/projects/notes", summary="Get project notes")]
pub(crate) async fn projects_notes_get(Query(q): Query<NotesQs>) -> impl IntoResponse {
    if let Some(p) = paths::project_notes_path(&q.proj) {
        if let Ok(bytes) = afs::read(&p).await {
            if let Ok(s) = String::from_utf8(bytes) {
                return s;
            }
        }
    }
    String::new()
}

#[arw_admin(method="POST", path="/admin/projects/notes", summary="Set project notes")]
pub(crate) async fn projects_notes_set(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(q): Query<NotesQs>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let Some(p) = paths::project_notes_path(&q.proj) else {
        return ApiError::bad_request("invalid proj").into_response();
    };
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    if let Err(e) = io::save_bytes_atomic(&p, &body).await {
        return ApiError::internal(&e.to_string()).into_response();
    }
    let mut p = json!({"name": q.proj});
    corr::ensure_corr(&mut p);
    state.bus.publish("Projects.NotesSaved", &p);
    super::ok(json!({})).into_response()
}
