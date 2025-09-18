use std::collections::{HashMap, VecDeque};

/// Small O(1) LRU map for deduplicating SSE events by a stable key.
/// Stores only the last `cap` keys with their DB ids.
#[derive(Debug, Default)]
pub(crate) struct SseIdCache {
    cap: usize,
    order: VecDeque<u64>,
    map: HashMap<u64, i64>,
}

impl SseIdCache {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            cap,
            ..Default::default()
        }
    }

    #[allow(clippy::map_entry)]
    pub(crate) fn insert(&mut self, key: u64, id: i64) {
        if self.map.contains_key(&key) {
            // Update existing; keep order as latest by pushing again
            self.map.insert(key, id);
            self.order.retain(|k| *k != key);
        } else if self.order.len() >= self.cap.max(1) {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
            self.map.insert(key, id);
        } else {
            self.map.insert(key, id);
        }
        self.order.push_back(key);
    }

    pub(crate) fn get(&self, key: u64) -> Option<i64> {
        self.map.get(&key).copied()
    }
}
