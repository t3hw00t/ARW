use std::collections::{HashMap, HashSet};

use super::ModelsInflightEntry;

#[derive(Debug, Clone)]
pub enum HashGuardRole {
    Primary,
    Coalesced { primary: String },
}

#[derive(Default)]
pub(super) struct HashGuardState {
    by_sha: HashMap<String, HashGuardEntry>,
    model_to_sha: HashMap<String, String>,
}

#[derive(Default)]
struct HashGuardEntry {
    primary: String,
    followers: HashSet<String>,
}

impl HashGuardState {
    pub(super) fn register(&mut self, model_id: &str, sha: &str) -> HashGuardRole {
        match self.by_sha.get_mut(sha) {
            Some(entry) => {
                entry.followers.insert(model_id.to_string());
                self.model_to_sha
                    .insert(model_id.to_string(), sha.to_string());
                HashGuardRole::Coalesced {
                    primary: entry.primary.clone(),
                }
            }
            None => {
                let entry = HashGuardEntry {
                    primary: model_id.to_string(),
                    followers: HashSet::new(),
                };
                self.by_sha.insert(sha.to_string(), entry);
                self.model_to_sha
                    .insert(model_id.to_string(), sha.to_string());
                HashGuardRole::Primary
            }
        }
    }

    pub(super) fn release_primary(&mut self, model_id: &str) -> Vec<String> {
        let Some(sha) = self.model_to_sha.remove(model_id) else {
            return Vec::new();
        };
        let Some(entry) = self.by_sha.remove(&sha) else {
            return Vec::new();
        };
        for follower in &entry.followers {
            self.model_to_sha.remove(follower);
        }
        entry.followers.into_iter().collect()
    }

    pub(super) fn release_model(&mut self, model_id: &str) {
        if let Some(sha) = self.model_to_sha.remove(model_id) {
            if let Some(entry) = self.by_sha.get_mut(&sha) {
                entry.followers.remove(model_id);
                if entry.primary == model_id {
                    let followers: Vec<_> = entry.followers.drain().collect();
                    self.by_sha.remove(&sha);
                    for follower in followers {
                        self.model_to_sha.remove(&follower);
                    }
                } else if entry.followers.is_empty() {
                    self.by_sha.remove(&sha);
                }
            }
        }
    }

    pub(super) fn progress_targets(&self, model_id: &str) -> Vec<String> {
        let mut targets = vec![model_id.to_string()];
        if let Some(sha) = self.model_to_sha.get(model_id) {
            if let Some(entry) = self.by_sha.get(sha) {
                if entry.primary == model_id {
                    targets.extend(entry.followers.iter().cloned());
                }
            }
        }
        targets
    }

    pub(super) fn inflight_snapshot(&self) -> Vec<ModelsInflightEntry> {
        self.by_sha
            .iter()
            .map(|(sha, entry)| ModelsInflightEntry {
                sha256: sha.clone(),
                primary: entry.primary.clone(),
                followers: entry.followers.iter().cloned().collect(),
                count: 1 + entry.followers.len() as u64,
            })
            .collect()
    }

    pub(super) fn followers_of_primary(&self, model_id: &str) -> Vec<String> {
        self.by_sha
            .values()
            .find(|entry| entry.primary == model_id)
            .map(|entry| entry.followers.iter().cloned().collect())
            .unwrap_or_default()
    }

}
