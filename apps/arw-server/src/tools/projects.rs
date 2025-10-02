use super::{ToolError, Value};
use crate::api::projects::{
    load_project_notes, write_project_notes, ProjectNotesReadError, ProjectNotesWriteError,
};
use crate::util;
use crate::AppState;
use chrono::{DateTime, SecondsFormat, Utc};
use pathdiff::diff_paths;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Deserialize)]
struct AppendNoteInput {
    project: String,
    #[serde(default)]
    screenshot_path: Option<String>,
    #[serde(default)]
    heading: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    timestamp: Option<bool>,
}

struct ScreenshotMeta {
    absolute: String,
    relative_to_project: Option<String>,
    relative_to_state: Option<String>,
    markdown_path: String,
}

pub(super) async fn append_note(state: &AppState, input: Value) -> Result<Value, ToolError> {
    let payload: AppendNoteInput = serde_json::from_value(input)
        .map_err(|err| ToolError::Invalid(format!("invalid input: {err}")))?;

    if payload.project.trim().is_empty() {
        return Err(ToolError::Invalid("missing project".into()));
    }

    let has_screenshot = payload
        .screenshot_path
        .as_ref()
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false);
    let has_note = payload
        .note
        .as_ref()
        .map(|n| !n.trim().is_empty())
        .unwrap_or(false);
    let has_markdown = payload
        .markdown
        .as_ref()
        .map(|m| !m.trim().is_empty())
        .unwrap_or(false);
    if !has_screenshot && !has_note && !has_markdown {
        return Err(ToolError::Invalid(
            "provide screenshot_path, note, or markdown to append".into(),
        ));
    }

    let doc = load_project_notes(&payload.project)
        .await
        .map_err(map_read_error)?;
    let project_root = util::state_dir().join("projects").join(&doc.proj);
    let notes_path = project_root.join("NOTES.md");
    let state_dir = util::state_dir();

    let screenshot_meta = if let Some(path) =
        payload
            .screenshot_path
            .as_ref()
            .and_then(|p| if p.trim().is_empty() { None } else { Some(p) })
    {
        let shot_path = PathBuf::from(path);
        let canonical = fs::canonicalize(&shot_path)
            .await
            .map_err(|err| ToolError::Invalid(format!("invalid screenshot_path: {err}")))?;
        let expected_base = state_dir.join("screenshots");
        if !canonical.starts_with(&expected_base) {
            return Err(ToolError::Invalid(
                "screenshot_path must reside under state_dir/screenshots".into(),
            ));
        }
        let absolute = normalize_path(&canonical);
        let rel_project = diff_paths(&canonical, &project_root).map(|p| normalize_path(&p));
        let rel_state = diff_paths(&canonical, &state_dir).map(|p| normalize_path(&p));
        let markdown_path = rel_project.clone().unwrap_or_else(|| absolute.clone());
        Some(ScreenshotMeta {
            absolute,
            relative_to_project: rel_project,
            relative_to_state: rel_state,
            markdown_path,
        })
    } else {
        None
    };

    let timestamp = Utc::now();
    let heading = select_heading(&payload, timestamp);
    let snippet = build_snippet(&payload, heading.as_deref(), &screenshot_meta, timestamp)?;

    let mut attempts = 0;
    let mut current_doc = doc;
    let save_result = loop {
        attempts += 1;
        let mut body = current_doc.content.clone();
        if !body.trim_end().is_empty() {
            if !body.ends_with('\n') {
                body.push('\n');
            }
            body.push('\n');
        }
        body.push_str(&snippet);
        if !body.ends_with('\n') {
            body.push('\n');
        }

        match write_project_notes(
            state,
            &current_doc.proj,
            body,
            current_doc.sha256.as_deref(),
        )
        .await
        {
            Ok(resp) => break resp,
            Err(ProjectNotesWriteError::ShaMismatch | ProjectNotesWriteError::MissingNotes)
                if attempts < 3 =>
            {
                current_doc = load_project_notes(&current_doc.proj)
                    .await
                    .map_err(map_read_error)?;
                continue;
            }
            Err(ProjectNotesWriteError::ShaMismatch)
            | Err(ProjectNotesWriteError::MissingNotes) => {
                return Err(ToolError::Runtime(
                    "notes changed during update; please retry".into(),
                ));
            }
            Err(ProjectNotesWriteError::InvalidProject) => {
                return Err(ToolError::Invalid("invalid project".into()))
            }
            Err(ProjectNotesWriteError::Io(err)) => {
                return Err(ToolError::Runtime(err.to_string()))
            }
        }
    };

    let mut response = json!({
        "ok": true,
        "proj": save_result.proj,
        "sha256": save_result.sha256,
        "bytes": save_result.bytes,
        "modified": save_result.modified,
        "corr_id": save_result.corr_id,
        "snippet": snippet,
        "notes_path": normalize_path(&notes_path),
        "timestamp_iso": timestamp.to_rfc3339_opts(SecondsFormat::Secs, true),
    });

    if let Some(heading) = heading {
        response["heading"] = json!(heading);
    }
    if let Some(meta) = screenshot_meta {
        response["screenshot"] = json!({
            "absolute": meta.absolute,
            "relative_to_project": meta.relative_to_project,
            "relative_to_state": meta.relative_to_state,
            "markdown_path": meta.markdown_path,
        });
    }

    Ok(response)
}

fn build_snippet(
    payload: &AppendNoteInput,
    heading: Option<&str>,
    screenshot: &Option<ScreenshotMeta>,
    timestamp: DateTime<Utc>,
) -> Result<String, ToolError> {
    let mut buf = String::new();
    if let Some(text) = heading {
        if !text.is_empty() {
            buf.push_str("## ");
            buf.push_str(text.trim());
            buf.push('\n');
        }
    }

    if let Some(meta) = screenshot {
        let alt = payload
            .caption
            .as_deref()
            .and_then(|s| {
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s.trim())
                }
            })
            .unwrap_or("Screenshot");
        buf.push_str(&format!(
            "![{}]({})\n",
            escape_brackets(alt),
            meta.markdown_path
        ));
    }

    if let Some(note) = payload.note.as_ref().and_then(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.trim())
        }
    }) {
        for line in note.lines() {
            buf.push_str("> ");
            buf.push_str(line.trim());
            buf.push('\n');
        }
    }

    if let Some(md) = payload.markdown.as_ref().and_then(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.trim())
        }
    }) {
        buf.push_str(md);
        if !md.ends_with('\n') {
            buf.push('\n');
        }
    }

    if buf.trim().is_empty() {
        // If nothing else, add timestamp note to avoid writing an empty section.
        buf.push_str(&format!(
            "Logged at {}\n",
            timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        ));
    }

    Ok(buf)
}

fn escape_brackets(input: &str) -> String {
    input.replace('[', "\\[").replace(']', "\\]")
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn select_heading(payload: &AppendNoteInput, now: DateTime<Utc>) -> Option<String> {
    if let Some(custom) = payload.heading.as_ref().and_then(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.trim().to_string())
        }
    }) {
        return Some(custom);
    }
    if payload.timestamp.unwrap_or(true) {
        Some(format!(
            "Screenshot {}",
            now.format("%Y-%m-%d %H:%M:%S UTC")
        ))
    } else {
        None
    }
}

fn map_read_error(err: ProjectNotesReadError) -> ToolError {
    match err {
        ProjectNotesReadError::InvalidProject => ToolError::Invalid("invalid project".into()),
        ProjectNotesReadError::InvalidUtf8 => {
            ToolError::Runtime("notes file is not valid UTF-8".into())
        }
        ProjectNotesReadError::Io(e) => ToolError::Runtime(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use arw_policy::PolicyEngine;
    use arw_wasi::ToolHost;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn append_note_appends_markdown_section() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_guard = crate::test_support::begin_state_env(temp.path());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);

        let state = AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(8)
            .build()
            .await;

        let project_dir = util::state_dir().join("projects").join("demo");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        std::fs::write(project_dir.join("NOTES.md"), "# Demo\n").expect("seed notes");

        let request = json!({
            "project": "demo",
            "heading": "Review",
            "note": "Observation ready",
            "timestamp": false
        });

        let output = append_note(&state, request).await.expect("append note");
        assert!(output["ok"].as_bool().unwrap_or(false));
        let notes = std::fs::read_to_string(project_dir.join("NOTES.md")).expect("read notes");
        assert!(notes.contains("## Review"));
        assert!(notes.contains("> Observation ready"));

        drop(env_guard);
    }

    #[tokio::test]
    async fn append_note_includes_relative_screenshot_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let env_guard = crate::test_support::begin_state_env(temp.path());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);

        let state = AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(8)
            .build()
            .await;

        let project_dir = util::state_dir().join("projects").join("demo");
        std::fs::create_dir_all(&project_dir).expect("create project dir");

        let shots_dir = util::state_dir()
            .join("screenshots")
            .join("2025")
            .join("10")
            .join("03");
        std::fs::create_dir_all(&shots_dir).expect("create screenshots dir");
        let shot_path = shots_dir.join("sample.png");
        std::fs::write(&shot_path, b"fake").expect("write screenshot");

        let request = json!({
            "project": "demo",
            "screenshot_path": shot_path.to_string_lossy(),
            "caption": "Example",
            "timestamp": false
        });

        let output = append_note(&state, request).await.expect("append note");
        let snippet = output["snippet"].as_str().expect("snippet");
        assert!(snippet.contains("![Example](../../screenshots"));
        let notes = std::fs::read_to_string(project_dir.join("NOTES.md")).expect("read notes");
        assert!(notes.contains("../../screenshots"));
        assert_eq!(
            output["screenshot"]["relative_to_project"].as_str(),
            Some("../../screenshots/2025/10/03/sample.png")
        );

        drop(env_guard);
    }
}
