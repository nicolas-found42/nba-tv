//! YouTube `search.list` quota budget (rung 2).
//!
//! Per ladder v2 §3 the only quota'd surface is YouTube search: 100
//! calls/day by default. Rungs 0–1 are quota-free; rungs 3–5 are
//! search-engine queries, keyless APIs, and static-HTML crawls. Sweep
//! priority (Finals/playoffs first, decades newest→oldest) decides *which*
//! queries get the budget, not this type — this type only caps the count.

/// Daily budget for YouTube `search.list` calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YoutubeQuota {
    daily_limit: u32,
    used: u32,
}

/// Default YouTube `search.list` budget: 100 calls/day.
pub const YOUTUBE_DAILY_LIMIT: u32 = 100;

impl YoutubeQuota {
    /// Fresh budget for a new day (100 calls).
    pub fn new() -> Self {
        Self {
            daily_limit: YOUTUBE_DAILY_LIMIT,
            used: 0,
        }
    }

    /// Budget with a custom daily limit (for tests / raised quotas).
    pub fn with_limit(daily_limit: u32) -> Self {
        Self {
            daily_limit,
            used: 0,
        }
    }

    /// Calls already scheduled today.
    pub fn used(&self) -> u32 {
        self.used
    }

    /// Calls still available today.
    pub fn remaining(&self) -> u32 {
        self.daily_limit.saturating_sub(self.used)
    }

    /// Schedule up to `requested` queries; returns how many were granted
    /// (capped at the remaining budget) and consumes them.
    pub fn schedule(&mut self, requested: u32) -> u32 {
        let granted = requested.min(self.remaining());
        self.used += granted;
        granted
    }

    /// Reset for a new day.
    pub fn reset(&mut self) {
        self.used = 0;
    }
}

impl Default for YoutubeQuota {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_caps_a_250_query_day_to_100() {
        let mut quota = YoutubeQuota::new();
        assert_eq!(quota.schedule(250), 100);
        assert_eq!(quota.used(), 100);
        assert_eq!(quota.remaining(), 0);
        // Nothing left once the day is spent.
        assert_eq!(quota.schedule(10), 0);
    }

    #[test]
    fn quota_grants_small_requests_in_full_and_resets_daily() {
        let mut quota = YoutubeQuota::new();
        assert_eq!(quota.schedule(40), 40);
        assert_eq!(quota.schedule(40), 40);
        assert_eq!(quota.remaining(), 20);
        quota.reset();
        assert_eq!(quota.remaining(), 100);
        assert_eq!(quota.schedule(100), 100);
    }
}
