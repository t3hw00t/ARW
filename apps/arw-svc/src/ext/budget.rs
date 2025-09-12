use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum BudgetClass {
    Interactive,
    Batch,
}

impl BudgetClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetClass::Interactive => "interactive",
            BudgetClass::Batch => "batch",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Budget {
    pub soft_ms: u64,
    pub hard_ms: u64,
    pub started: Instant,
    pub class: BudgetClass,
}

impl Budget {
    pub fn new(soft_ms: u64, hard_ms: u64, class: BudgetClass) -> Self {
        Self {
            soft_ms,
            hard_ms,
            started: Instant::now(),
            class,
        }
    }

    pub fn for_download() -> Self {
        // Defaults are conservative: 0 means "unbounded" from budget's perspective
        // and we don't apply a timeout unless explicitly configured.
        let soft_ms = std::env::var("ARW_BUDGET_DOWNLOAD_SOFT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let hard_ms = std::env::var("ARW_BUDGET_DOWNLOAD_HARD_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Self::new(soft_ms, hard_ms, BudgetClass::Batch)
    }

    pub fn spent_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn remaining_soft_ms(&self) -> u64 {
        if self.soft_ms == 0 {
            u64::MAX
        } else {
            self.soft_ms.saturating_sub(self.spent_ms())
        }
    }

    pub fn remaining_hard_ms(&self) -> u64 {
        if self.hard_ms == 0 {
            u64::MAX
        } else {
            self.hard_ms.saturating_sub(self.spent_ms())
        }
    }

    pub fn hard_exhausted(&self) -> bool {
        self.hard_ms > 0 && self.spent_ms() >= self.hard_ms
    }

    pub fn request_timeout(&self) -> Option<Duration> {
        if self.hard_ms == 0 {
            None
        } else {
            let ms = self.remaining_hard_ms().max(1);
            Some(Duration::from_millis(ms))
        }
    }

    pub fn apply_to_request(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let rb = rb
            .header("x-budget-soft-ms", self.soft_ms.to_string())
            .header("x-budget-hard-ms", self.hard_ms.to_string())
            .header("x-budget-class", self.class.as_str())
            .header(
                "x-budget-started-ms",
                self.started.elapsed().as_millis().to_string(),
            );
        if let Some(d) = self.request_timeout() {
            rb.timeout(d)
        } else {
            rb
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "soft_ms": self.soft_ms,
            "hard_ms": self.hard_ms,
            "spent_ms": self.spent_ms(),
            "remaining_soft_ms": self.remaining_soft_ms(),
            "remaining_hard_ms": self.remaining_hard_ms(),
            "class": self.class.as_str(),
        })
    }
}
