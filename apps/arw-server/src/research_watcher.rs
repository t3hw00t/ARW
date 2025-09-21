use anyhow::{Context, Result};
use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

use crate::{http_timeout, tasks::TaskHandle, AppState};
use arw_topics as topics;

const MIN_INTERVAL_SECS: u64 = 300;
const DEFAULT_INTERVAL_SECS: u64 = 900;

#[derive(Debug, Clone, Deserialize)]
struct SeedItem {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

pub fn start(state: AppState) -> Vec<TaskHandle> {
    if !state.kernel_enabled() {
        return Vec::new();
    }
    let client = reqwest::Client::builder()
        .timeout(http_timeout::get_duration())
        .build()
        .ok();
    vec![TaskHandle::new(
        "research_watcher.poller",
        tokio::spawn(async move {
            loop {
                if let Err(err) = sync_once(&state, client.as_ref()).await {
                    warn!(target: "research_watcher", "sync error: {err:?}");
                }
                let interval = interval_secs();
                sleep(Duration::from_secs(interval)).await;
            }
        }),
    )]
}

fn interval_secs() -> u64 {
    std::env::var("ARW_RESEARCH_WATCHER_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| v.max(MIN_INTERVAL_SECS))
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

async fn sync_once(state: &AppState, client: Option<&reqwest::Client>) -> Result<()> {
    let mut items = Vec::new();
    items.extend(load_seed_items_from_file()?);
    if let Some(cli) = client {
        items.extend(load_items_from_feeds(cli).await?);
    }
    if items.is_empty() {
        return Ok(());
    }
    let mut ingested = 0u64;
    for item in items {
        let id = state
            .kernel()
            .upsert_research_watcher_item_async(
                item.source.clone(),
                item.source_id.clone(),
                item.title.clone(),
                item.summary.clone(),
                item.url.clone(),
                item.payload.clone(),
            )
            .await?;
        debug!(target: "research_watcher", "upserted item {id}");
        ingested += 1;
    }
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    state.bus().publish(
        topics::TOPIC_RESEARCH_WATCHER_UPDATED,
        &serde_json::json!({
            "ingested": ingested,
            "time": now,
        }),
    );
    Ok(())
}

fn load_seed_items_from_file() -> Result<Vec<SeedItem>> {
    let path = match std::env::var("ARW_RESEARCH_WATCHER_SEED") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => return Ok(Vec::new()),
    };
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("reading research watcher seed from {path}"))?;
    parse_items(data).with_context(|| format!("parsing research watcher seed {path}"))
}

async fn load_items_from_feeds(client: &reqwest::Client) -> Result<Vec<SeedItem>> {
    let feeds_env = match std::env::var("ARW_RESEARCH_WATCHER_FEEDS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for feed in feeds_env.split(',') {
        let feed = feed.trim();
        if feed.is_empty() {
            continue;
        }
        match client.get(feed).send().await {
            Ok(resp) => match resp.text().await {
                Ok(body) => match parse_items(body) {
                    Ok(mut items) => out.append(&mut items),
                    Err(err) => warn!(target: "research_watcher", "parse feed {feed} error: {err}"),
                },
                Err(err) => warn!(target: "research_watcher", "feed {feed} body error: {err}"),
            },
            Err(err) => warn!(target: "research_watcher", "fetch feed {feed} error: {err}"),
        }
    }
    Ok(out)
}

fn parse_items(body: impl AsRef<str>) -> Result<Vec<SeedItem>> {
    let text = body.as_ref();
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(text)?;
    if value.is_array() {
        serde_json::from_value(value).map_err(Into::into)
    } else if let Some(items) = value.get("items") {
        serde_json::from_value(items.clone()).map_err(Into::into)
    } else {
        Ok(Vec::new())
    }
}

pub async fn update_status(
    state: &AppState,
    id: &str,
    status: &str,
    note: Option<String>,
) -> Result<Option<arw_kernel::ResearchWatcherItem>> {
    let changed = state
        .kernel()
        .update_research_watcher_status_async(id.to_string(), status.to_string(), note.clone())
        .await?;
    if !changed {
        return Ok(None);
    }
    let item = state
        .kernel()
        .get_research_watcher_item_async(id.to_string())
        .await?;
    if let Some(ref it) = item {
        let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        state.bus().publish(
            topics::TOPIC_RESEARCH_WATCHER_UPDATED,
            &serde_json::json!({
                "id": it.id,
                "status": status,
                "time": now,
            }),
        );
    }
    Ok(item)
}
