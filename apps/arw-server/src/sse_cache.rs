use std::collections::{HashMap, VecDeque};

/// Small O(1) LRU map for deduplicating SSE events by a stable key.
/// Stores only the last `cap` keys with their DB ids.
#[derive(Debug)]
pub(crate) struct SseIdCache {
    cap: usize,
    order: VecDeque<(u64, u64)>,
    map: HashMap<u64, (i64, u64)>,
    next_token: u64,
}

impl Default for SseIdCache {
    fn default() -> Self {
        Self {
            cap: 0,
            order: VecDeque::new(),
            map: HashMap::new(),
            next_token: 0,
        }
    }
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
        if self.cap == 0 {
            self.map.clear();
            self.order.clear();
            return;
        }
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        self.map.insert(key, (id, token));
        self.order.push_back((key, token));
        self.prune_stale();
        self.evict_to_capacity();
        self.prune_stale();
    }

    pub(crate) fn get(&self, key: u64) -> Option<i64> {
        self.map.get(&key).map(|(id, _)| *id)
    }

    fn prune_stale(&mut self) {
        while let Some((front_key, front_token)) = self.order.front().copied() {
            match self.map.get(&front_key) {
                Some((_, current_token)) if *current_token == front_token => break,
                Some(_) | None => {
                    self.order.pop_front();
                }
            }
        }
    }

    fn evict_to_capacity(&mut self) {
        let cap = self.cap.max(1);
        while self.map.len() > cap {
            if let Some((old_key, old_token)) = self.order.pop_front() {
                if let Some((_, current_token)) = self.map.get(&old_key) {
                    if *current_token == old_token {
                        self.map.remove(&old_key);
                    }
                }
            } else {
                break;
            }
        }
    }
}
