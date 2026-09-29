//! One politeness configuration point every probe reads.
//!
//! Real probes (later tickets) implement pacing against these values instead
//! of inventing their own: the Internet Archive probe spaces requests by
//! [`PolitenessConfig::ia_request_min_interval`], the yt-dlp probe sleeps
//! [`PolitenessConfig::ytdlp_request_sleep`] every
//! [`PolitenessConfig::ytdlp_sleep_requests`] requests, and the YouTube probe
//! spends [`PolitenessConfig::youtube_daily_limit`] through the existing
//! [`YoutubeQuota`] tracker (see [`PolitenessConfig::youtube_quota`]).
//! [`PolitenessConfig::max_concurrent_probes`] caps parallelism; the sweep
//! itself is sequential today (`1`), so concurrent probes must still honor it
//! when they arrive.

use nbatv_ladder::YoutubeQuota;
use std::time::Duration;

/// The single source probes read for pacing, sleep, and budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolitenessConfig {
    /// Minimum spacing between Internet Archive requests (rung 1).
    pub ia_request_min_interval: Duration,
    /// Sleep inserted between yt-dlp requests (rungs 2–3).
    pub ytdlp_request_sleep: Duration,
    /// Insert the sleep after every N yt-dlp requests (yt-dlp
    /// `--sleep-requests` semantics).
    pub ytdlp_sleep_requests: u32,
    /// How many probes may run concurrently. The sweep is sequential, so
    /// real probes must still read this before parallelizing.
    pub max_concurrent_probes: usize,
    /// Daily YouTube `search.list` budget (rung 2). The value the
    /// [`YoutubeQuota`] tracker is constructed with — the tracker itself is
    /// threaded through the sweep, not copied into the config, so spend is
    /// shared across rungs and retries.
    pub youtube_daily_limit: u32,
}

impl Default for PolitenessConfig {
    fn default() -> Self {
        Self {
            ia_request_min_interval: Duration::from_secs(2),
            ytdlp_request_sleep: Duration::from_secs(5),
            ytdlp_sleep_requests: 7,
            max_concurrent_probes: 1,
            youtube_daily_limit: nbatv_ladder::quota::YOUTUBE_DAILY_LIMIT,
        }
    }
}

impl PolitenessConfig {
    /// Fresh [`YoutubeQuota`] tracker for this config's daily budget.
    /// Call once per sweep day and hand the tracker to [`crate::sweep_game`].
    pub fn youtube_quota(&self) -> YoutubeQuota {
        YoutubeQuota::with_limit(self.youtube_daily_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_documented_slots() {
        let cfg = PolitenessConfig::default();
        assert_eq!(cfg.ia_request_min_interval, Duration::from_secs(2));
        assert_eq!(cfg.ytdlp_request_sleep, Duration::from_secs(5));
        assert_eq!(cfg.ytdlp_sleep_requests, 7);
        assert_eq!(cfg.max_concurrent_probes, 1);
        assert_eq!(
            cfg.youtube_daily_limit,
            nbatv_ladder::quota::YOUTUBE_DAILY_LIMIT
        );
    }

    #[test]
    fn quota_constructor_spends_the_configured_budget() {
        let mut quota = PolitenessConfig {
            youtube_daily_limit: 3,
            ..PolitenessConfig::default()
        }
        .youtube_quota();
        assert_eq!(quota.schedule(5), 3);
        assert_eq!(quota.remaining(), 0);
    }
}
