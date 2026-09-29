//! One politeness configuration point every probe reads.
//!
//! Real probes implement pacing against these values instead of inventing
//! their own: the Internet Archive probe spaces requests by
//! [`PolitenessConfig::ia_request_min_interval`], and the yt-dlp probe
//! sleeps [`PolitenessConfig::ytdlp_request_sleep`] between requests via
//! yt-dlp's own `--sleep-requests SECONDS` flag (verified against the
//! installed yt-dlp: there is no every-N-requests flag upstream, and
//! sleeping every request is at least as polite as every Nth). The YouTube
//! probe spends [`PolitenessConfig::youtube_daily_limit`] through the
//! existing [`YoutubeQuota`] tracker (see
//! [`PolitenessConfig::youtube_quota`]). The sweep itself is sequential
//! today; if probes ever run in parallel, a concurrency cap belongs here
//! so the config stays the single pacing point.

use nbatv_ladder::YoutubeQuota;
use std::time::Duration;

/// The single source probes read for pacing, sleep, and budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolitenessConfig {
    /// Minimum spacing between Internet Archive requests (rung 1).
    pub ia_request_min_interval: Duration,
    /// Sleep inserted between yt-dlp requests (rungs 2–3), passed as
    /// yt-dlp's `--sleep-requests` seconds.
    pub ytdlp_request_sleep: Duration,
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
