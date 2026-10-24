//! Retina windows can have a visible layer working set larger than 32 MiB.
//! Grow the Mac default from observed demand; explicit budgets stay authoritative.
use std::collections::HashMap;

use jag_draw::ExternalTextureId;

const DEFAULT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ADAPTIVE_BYTES: u64 = 128 * 1024 * 1024;

pub(super) struct RetainedBudget {
    bytes: u64,
    adaptive: bool,
    requested: HashMap<ExternalTextureId, u64>,
}

impl Default for RetainedBudget {
    fn default() -> Self {
        Self {
            bytes: DEFAULT_BYTES,
            adaptive: cfg!(target_os = "macos"),
            requested: HashMap::new(),
        }
    }
}

impl RetainedBudget {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn observe(&mut self, id: ExternalTextureId, charge: u64) {
        if self.adaptive {
            self.requested
                .entry(id)
                .and_modify(|old| *old = (*old).max(charge))
                .or_insert(charge);
        }
    }

    pub fn begin_frame(&mut self) {
        if self.adaptive {
            let demand = self
                .requested
                .values()
                .copied()
                .fold(0, u64::saturating_add);
            // Keep one quarter of headroom for newly exposed scroll tiles.
            // A high-water budget avoids reallocating on alternating frames;
            // LRU eviction still bounds residency, and clear resets the budget.
            let desired = demand.saturating_add(demand / 4).min(MAX_ADAPTIVE_BYTES);
            self.bytes = self.bytes.max(desired);
        }
        self.requested.clear();
    }

    pub fn clear(&mut self) {
        self.requested.clear();
        if self.adaptive {
            self.bytes = DEFAULT_BYTES;
        }
    }

    pub fn set(&mut self, bytes: u64) {
        self.adaptive = false;
        self.bytes = bytes;
        self.requested.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adaptive() -> RetainedBudget {
        RetainedBudget {
            adaptive: true,
            ..Default::default()
        }
    }

    #[test]
    fn working_set_is_deduplicated_and_gets_scroll_headroom() {
        let mut budget = adaptive();
        for _ in 0..4 {
            budget.observe(ExternalTextureId(1), 40 * 1024 * 1024);
        }
        budget.begin_frame();
        assert_eq!(budget.bytes(), 50 * 1024 * 1024);
        budget.begin_frame();
        assert_eq!(budget.bytes(), 50 * 1024 * 1024);
        budget.clear();
        assert_eq!(budget.bytes(), DEFAULT_BYTES);
    }

    #[test]
    fn oversized_working_set_is_bounded_without_overflow() {
        let mut budget = adaptive();
        budget.observe(ExternalTextureId(1), u64::MAX);
        budget.observe(ExternalTextureId(2), u64::MAX);
        budget.begin_frame();
        assert_eq!(budget.bytes(), MAX_ADAPTIVE_BYTES);
    }

    #[test]
    fn explicit_budgets_including_disabled_never_grow() {
        for bytes in [0, 4096, DEFAULT_BYTES] {
            let mut budget = adaptive();
            budget.set(bytes);
            budget.observe(ExternalTextureId(1), MAX_ADAPTIVE_BYTES);
            budget.begin_frame();
            budget.clear();
            assert_eq!(budget.bytes(), bytes);
        }
    }
}
