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
    for (slot, required) in summary.slot_budgets.iter() {
        if *required == 0 {
            continue;
        }
        let have = summary.slot_counts.get(slot).copied().unwrap_or(0);
        if have < (*required).min(summary.selected.max(1)) {
            reasons.push(format!("slot_underfilled:{slot}"));
        }
    }
    CoverageVerdict {
        needs_more: !reasons.is_empty(),
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::working_set::{WorkingSet, WorkingSetSummary};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn empty_ws(summary: WorkingSetSummary) -> WorkingSet {
        WorkingSet {
            items: Vec::new(),
            seeds: Vec::new(),
            expanded: Vec::new(),
            diagnostics: json!({}),
            summary,
        }
    }

    #[test]
    fn satisfied_when_targets_met() {
        let mut lanes = BTreeMap::new();
        lanes.insert("semantic".to_string(), 3usize);
        lanes.insert("procedural".to_string(), 2usize);
        let summary = WorkingSetSummary {
            target_limit: 5,
            lanes_requested: 2,
            selected: 5,
            avg_cscore: 0.75,
            max_cscore: 0.82,
            min_cscore: 0.6,
            threshold_hits: 3,
            total_candidates: 7,
            lane_counts: lanes,
            slot_counts: BTreeMap::new(),
            slot_budgets: BTreeMap::new(),
            min_score: 0.6,
            scorer: "mmrd".into(),
        };
        let verdict = assess(&empty_ws(summary));
        assert!(!verdict.needs_more);
        assert!(verdict.reasons.is_empty());
    }

    #[test]
    fn flags_common_coverage_gaps() {
        let mut lanes = BTreeMap::new();
        lanes.insert("semantic".to_string(), 4usize);
        let summary = WorkingSetSummary {
            target_limit: 8,
            lanes_requested: 3,
            selected: 3,
            avg_cscore: 0.32,
            max_cscore: 0.35,
            min_cscore: 0.1,
            threshold_hits: 0,
            total_candidates: 12,
            lane_counts: lanes,
            slot_counts: BTreeMap::new(),
            slot_budgets: BTreeMap::new(),
            min_score: 0.6,
            scorer: "mmrd".into(),
        };
        let verdict = assess(&empty_ws(summary));
        let reasons: std::collections::HashSet<_> =
            verdict.reasons.iter().map(|s| s.as_str()).collect();
        assert!(verdict.needs_more);
        assert!(reasons.contains("below_target_limit"));
        assert!(reasons.contains("low_lane_diversity"));
        assert!(reasons.contains("weak_average_score"));
        assert!(reasons.contains("no_items_above_threshold"));
    }

    #[test]
    fn slot_budget_gap_surfaces_reason() {
        let mut budgets = BTreeMap::new();
        budgets.insert("instructions".to_string(), 1usize);
        let summary = WorkingSetSummary {
            target_limit: 4,
            lanes_requested: 2,
            selected: 2,
            avg_cscore: 0.6,
            max_cscore: 0.7,
            min_cscore: 0.5,
            threshold_hits: 2,
            total_candidates: 5,
            lane_counts: BTreeMap::new(),
            slot_counts: BTreeMap::new(),
            slot_budgets: budgets,
            min_score: 0.4,
            scorer: "mmrd".into(),
        };
        let verdict = assess(&empty_ws(summary));
        assert!(verdict.needs_more);
        assert!(verdict
            .reasons
            .iter()
            .any(|r| r == "slot_underfilled:instructions"));
    }
}
