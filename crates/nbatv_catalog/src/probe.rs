//! The [`SourceProbe`] port: the ONE seam network lookups sit behind.
//!
//! Probes are per-rung: one probe answers one rung for one game. A probe
//! returns the recorded query text it used plus candidate evidence
//! (`url_or_pointer` with the title/description/duration the scorer needs).
//! Scoring lives in the catalog ([`crate::score_candidate`]), never in
//! probes, so every probe — fake or real — is judged by the same rule.
//!
//! Registration: collect `&dyn SourceProbe` refs into a [`ProbeRegistry`]
//! (keyed by [`SourceProbe::rung`], last wins) and hand it to
//! [`crate::sweep_game`]. Later tickets implement one struct per rung
//! against this trait:
//!
//! ```ignore
//! pub struct IaProbe;
//! impl SourceProbe for IaProbe {
//!     fn rung(&self) -> u8 { 1 }
//!     fn name(&self) -> &'static str { "internet-archive" }
//!     fn probe(&self, game: &GameContext, politeness: &PolitenessConfig,
//!              quota: &mut YoutubeQuota) -> ProbeOutcome {
//!         std::thread::sleep(politeness.ia_request_min_interval);
//!         // ... network lookup here, offline fakes in tests ...
//!         ProbeOutcome::found(query_text, candidates)
//!     }
//! }
//! ```
//!
//! Quota contract (rung 2): the YouTube probe spends one budget unit per
//! outbound query via `quota.schedule(1)` and returns
//! [`ProbeOutcome::deferred`] when nothing is granted. A deferred rung
//! records nothing, so the next sweep retries it instead of burning the
//! 90-day rescan window.

use crate::politeness::PolitenessConfig;
use nbatv_ladder::YoutubeQuota;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// The game a probe queries for: identity plus the teams/date probes need
/// to form queries and the scorer needs to judge evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameContext {
    /// Basketball-Reference box-score slug, e.g. `194611010TRH`.
    pub game_id: String,
    /// Home team slug, e.g. `TRH`.
    pub home_team: String,
    /// Away team slug, e.g. `NYK`.
    pub away_team: String,
    /// ISO date, e.g. `1946-11-01`.
    pub date: String,
}

impl GameContext {
    /// Lowercase team tokens the scorer matches against evidence text.
    pub fn team_tokens(&self) -> [String; 2] {
        [self.home_team.to_lowercase(), self.away_team.to_lowercase()]
    }
}

/// One candidate a probe found: where it lives plus the evidence text the
/// catalog scorer judges. No verdict here — scoring is the catalog's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeCandidate {
    /// Stream URL or existence pointer for the candidate.
    pub url_or_pointer: String,
    /// Candidate title, as returned by the source.
    pub title: String,
    /// Candidate description, as returned by the source.
    pub description: String,
    /// Duration in seconds, when the source reports one.
    pub duration_secs: Option<u64>,
}

/// What one probe call produced for one rung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// The exact outbound query text, recorded as sweep evidence.
    pub query_text: String,
    /// Candidates found, in source order. Empty means the query ran and
    /// found nothing (recorded as a Reject rung, consuming the rung).
    pub candidates: Vec<ProbeCandidate>,
    /// The query could not run (spent quota, politeness backoff): the sweep
    /// records nothing and retries the rung next time.
    pub deferred: bool,
}

impl ProbeOutcome {
    /// A query that ran, with the candidates it found (possibly none).
    pub fn found(query_text: String, candidates: Vec<ProbeCandidate>) -> Self {
        Self {
            query_text,
            candidates,
            deferred: false,
        }
    }

    /// A query that ran and found nothing.
    pub fn empty(query_text: String) -> Self {
        Self::found(query_text, Vec::new())
    }

    /// The query could not run: records nothing, retries next sweep.
    pub fn deferred(query_text: String) -> Self {
        Self {
            query_text,
            candidates: Vec::new(),
            deferred: true,
        }
    }
}

/// One rung's network lookup behind the port. Implementations are offline
/// in tests (fakes) and perform pacing + lookup in production.
pub trait SourceProbe {
    /// The ladder rung (0–4) this probe answers. Other rungs ignore it.
    fn rung(&self) -> u8;
    /// Human name for logs and diagnostics (usually the rung name).
    fn name(&self) -> &'static str;
    /// Answer one rung for one game. Reads pacing from `politeness`,
    /// spends YouTube budget through `quota` (rung 2), and returns the
    /// recorded query plus candidate evidence — never a verdict.
    fn probe(
        &self,
        game: &GameContext,
        politeness: &PolitenessConfig,
        quota: &mut YoutubeQuota,
    ) -> ProbeOutcome;
}

/// The probes one sweep consults, keyed by rung. Register one probe per
/// rung 0–4; rungs with no probe stay unswept (Sweeping, never absent).
#[derive(Default)]
pub struct ProbeRegistry<'a> {
    probes: HashMap<u8, &'a dyn SourceProbe>,
}

impl<'a> ProbeRegistry<'a> {
    /// Empty registry: every rung stays unswept until registered.
    pub fn new() -> Self {
        Self {
            probes: HashMap::new(),
        }
    }

    /// Register a probe under its [`SourceProbe::rung`]; a second probe
    /// for the same rung replaces the first.
    pub fn register(&mut self, probe: &'a dyn SourceProbe) -> &mut Self {
        self.probes.insert(probe.rung(), probe);
        self
    }

    /// The probe for one rung, if registered.
    pub fn get(&self, rung: u8) -> Option<&'a dyn SourceProbe> {
        self.probes.get(&rung).copied()
    }
}

/// Scripted fake probe for offline sweeps: replays one fixed outcome,
/// counts calls, and captures the politeness config it was given so tests
/// can assert the config reaches probes untouched. Production later tickets
/// replace this rung-by-rung with real probes; the offline demo keeps it.
pub struct ScriptedProbe {
    rung: u8,
    outcome: ProbeOutcome,
    calls: Cell<usize>,
    last_politeness: RefCell<Option<PolitenessConfig>>,
    last_quota_remaining: RefCell<Option<u32>>,
}

impl ScriptedProbe {
    /// Fake answering `rung` with `outcome` on every call.
    pub fn responding(rung: u8, outcome: ProbeOutcome) -> Self {
        Self {
            rung,
            outcome,
            calls: Cell::new(0),
            last_politeness: RefCell::new(None),
            last_quota_remaining: RefCell::new(None),
        }
    }

    /// The YouTube budget remaining at the most recent call, if any: two
    /// probes reporting the same value shared one quota (the runner's
    /// one-quota-per-run shape).
    pub fn last_quota_remaining(&self) -> Option<u32> {
        *self.last_quota_remaining.borrow()
    }

    /// How many times the sweep called this probe.
    pub fn calls(&self) -> usize {
        self.calls.get()
    }

    /// The politeness config from the most recent call, if any.
    pub fn last_politeness(&self) -> Option<PolitenessConfig> {
        self.last_politeness.borrow().clone()
    }
}

impl SourceProbe for ScriptedProbe {
    fn rung(&self) -> u8 {
        self.rung
    }

    fn name(&self) -> &'static str {
        "scripted-probe"
    }

    fn probe(
        &self,
        _game: &GameContext,
        politeness: &PolitenessConfig,
        quota: &mut YoutubeQuota,
    ) -> ProbeOutcome {
        self.calls.set(self.calls.get() + 1);
        *self.last_politeness.borrow_mut() = Some(politeness.clone());
        *self.last_quota_remaining.borrow_mut() = Some(quota.remaining());
        self.outcome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keys_probes_by_rung_last_wins() {
        let a = ScriptedProbe::responding(1, ProbeOutcome::empty("a".to_owned()));
        let b = ScriptedProbe::responding(1, ProbeOutcome::empty("b".to_owned()));
        let mut reg = ProbeRegistry::new();
        reg.register(&a).register(&b);
        assert_eq!(reg.get(1).unwrap().name(), "scripted-probe");
        assert!(reg.get(0).is_none());
        // Last registration wins the rung.
        assert_eq!(
            reg.get(1).unwrap().probe(
                &GameContext {
                    game_id: "g".to_owned(),
                    home_team: "h".to_owned(),
                    away_team: "a".to_owned(),
                    date: "1946-11-01".to_owned(),
                },
                &PolitenessConfig::default(),
                &mut YoutubeQuota::new(),
            ),
            ProbeOutcome::empty("b".to_owned())
        );
        assert_eq!(a.calls(), 0);
        assert_eq!(b.calls(), 1);
    }
}
