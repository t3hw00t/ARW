use crate::working_set::WorkingSet;

#[derive(Debug, Clone)]
pub struct CoverageVerdict {
    pub needs_more: bool,
    pub reasons: Vec<String>,
}

impl CoverageVerdict {
    pub fn satisfied() -> Self {
        CoverageVerdict {
            needs_more: false,
            reasons: Vec::new(),
        }
    }
}

#[allow(dead_code)]
pub fn needs_more_context(ws: &WorkingSet) -> bool {
    assess(ws).needs_more
}

pub fn assess(ws: &WorkingSet) -> CoverageVerdict {
    let mut reasons: Vec<String> = Vec::new();
    let summary = &ws.summary;
    if summary.selected == 0 {
        reasons.push("no_items_selected".to_string());
    }
    if summary.selected < ((summary.target_limit as f32 * 0.6).ceil() as usize) {
        reasons.push("below_target_limit".to_string());
    }
    let desired_lanes = summary.lanes_requested.clamp(1, 3);
    if summary.lane_counts.len() < desired_lanes.min(2) {
        reasons.push("low_lane_diversity".to_string());
    }
    if summary.avg_cscore < (summary.min_score * 0.9) {
        reasons.push("weak_average_score".to_string());
    }
    if summary.threshold_hits == 0 && summary.max_cscore < summary.min_score {
        reasons.push("no_items_above_threshold".to_string());
    }
    CoverageVerdict {
        needs_more: !reasons.is_empty(),
        reasons,
    }
}
