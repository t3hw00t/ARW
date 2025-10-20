mod builder;
mod models;
mod spec;

pub use builder::{assemble, assemble_with_observer};
pub use models::{
    BusObserver, ChannelObserver, CompositeObserver, WorkingSet, WorkingSetStreamEvent,
    WorkingSetSummary,
};
#[allow(unused_imports)]
pub use spec::{
    default_diversity_lambda, default_expand_per_seed, default_expand_query,
    default_expand_query_top_k, default_lane_bonus, default_lanes, default_limit,
    default_max_iterations, default_min_score, default_scorer, default_streaming_enabled,
    WorkingSetSpec,
};

pub const STREAM_EVENT_STARTED: &str = topics::TOPIC_WORKING_SET_STARTED;
pub const STREAM_EVENT_SEED: &str = topics::TOPIC_WORKING_SET_SEED;
pub const STREAM_EVENT_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPANDED;
pub const STREAM_EVENT_QUERY_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPAND_QUERY;
pub const STREAM_EVENT_SELECTED: &str = topics::TOPIC_WORKING_SET_SELECTED;
pub const STREAM_EVENT_COMPLETED: &str = topics::TOPIC_WORKING_SET_COMPLETED;

const METRIC_WORLD_CANDIDATES: &str = "arw_context_world_candidates_total";
const DEFAULT_WORLD_LANE: &str = "world";

type SharedValue = Arc<Value>;

use std::sync::Arc;

use serde_json::Value;

use arw_topics as topics;

#[cfg(test)]
mod tests;
