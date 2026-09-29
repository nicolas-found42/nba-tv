//! Fetch pipeline: one polite, resumable season batch at a time.
//!
//! `fetch_season` pulls every page of a single season through a [`FetchClient`]
//! (real HTTP in production, canned HTML in tests — no network here), spaces
//! requests by [`FETCH_MIN_INTERVAL`], and stores each body as gzip at
//! `out_dir.join(raw_snapshot_path(source, season, file_name))`. Files already
//! on disk are skipped without touching the network, so a killed run resumes
//! where it stopped; per-page `meta-revised` stamps come back in the report
//! for the caller's freshness store (see [`crate::recrawl_hint`]).
//!
//! The snapshot codec (hand-rolled CRC-32 + stored-block gzip, lines kept
//! together with the writers they serve) lives in this module too.

use std::fmt;
use std::path::Path;

use crate::br_html::{attr_value, is_tag_open, tag_end};
use crate::{raw_snapshot_path, PageRevision};

// ---------------------------------------------------------------------------
// Fetch pipeline: one polite, resumable season batch at a time
// ---------------------------------------------------------------------------
//
// `fetch_season` pulls every page of a single season through a [`FetchClient`]
// (real HTTP in production, canned HTML in tests — no network here), spaces
// requests by [`FETCH_MIN_INTERVAL`], and stores each body as gzip at
// `out_dir.join(raw_snapshot_path(source, season, file_name))`. Files already
// on disk are skipped without touching the network, so a killed run resumes
// where it stopped; per-page `meta-revised` stamps come back in the report
// for the caller's freshness store (see [`recrawl_hint`]).

use std::time::{Duration, Instant};

/// Minimum spacing between BR requests. robots.txt sets `Crawl-delay: 3`;
/// the extra half second is margin. One season batch runs at a time, so
/// this single constant paces the whole archive build (~75k box pages at
/// this rate ≈ 3 days, resumable per season).
pub const FETCH_MIN_INTERVAL: Duration = Duration::from_millis(3_500);

/// How long the pipeline must still wait given `elapsed` since the previous
/// request. Pure (never sleeps) so etiquette stays unit-testable.
pub fn etiquette_delay(elapsed: Duration) -> Duration {
    FETCH_MIN_INTERVAL.saturating_sub(elapsed)
}

/// Fetch-pipeline failure: bad path components, snapshot I/O, the client's
/// own fetch error, or undecodable snapshot bytes.
#[derive(Debug)]
pub enum FetchError {
    UnsafePath(String),
    Io(std::io::Error),
    Client(String),
    /// The page does not exist at the source (HTTP 404/410 from the live
    /// client). Distinct from [`FetchError::Client`] so a crawl driver can
    /// dead-pool a permanently missing page instead of retrying it.
    NotFound(String),
    /// The source answered "slow down" (HTTP 429/503): the crawl is
    /// exceeding its rate budget and must back off before retrying.
    Throttled(String),
    Decode(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::UnsafePath(s) => write!(f, "unsafe snapshot path component: {s}"),
            FetchError::Io(e) => write!(f, "snapshot I/O: {e}"),
            FetchError::Client(s) => write!(f, "fetch failed: {s}"),
            FetchError::NotFound(s) => write!(f, "page not found: {s}"),
            FetchError::Throttled(s) => write!(f, "throttled: {s}"),
            FetchError::Decode(s) => write!(f, "bad snapshot bytes: {s}"),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FetchError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Page source for [`fetch_season`]. Production use is a thin HTTP client;
/// tests substitute canned HTML. Returns the raw page body; the pipeline
/// derives freshness stamps and snapshot bytes itself.
///
/// The body MUST be transfer-decoded (identity) text: the client must not
/// hand back compressed bytes. In practice that means sending no
/// `Accept-Encoding: gzip` (BR then serves identity), or inflating the
/// response before returning — the `String` return type already forces
/// this, since compressed bytes are not valid UTF-8. Snapshot storage
/// re-compresses with [`gzip_encode`] itself, so the pipeline never meets
/// server-side dynamic-Huffman gzip: [`gzip_decode`] only ever reads back
/// what [`write_snapshot_gz`] wrote (plus `gzip -0`-shaped files).
pub trait FetchClient {
    fn fetch(&self, url: &str) -> Result<String, FetchError>;
}

/// One page of a season batch: destination file name under
/// `data/raw/{source}/{season}/`, its URL, the last `meta-revised` stamp
/// seen for it (freshness bookkeeping), and `force` to re-fetch even when
/// the snapshot is already on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchJob {
    pub file_name: String,
    pub url: String,
    pub last_seen_revision: Option<String>,
    pub force: bool,
}

impl FetchJob {
    pub fn new(file_name: &str, url: &str) -> Self {
        FetchJob {
            file_name: file_name.to_owned(),
            url: url.to_owned(),
            last_seen_revision: None,
            force: false,
        }
    }

    pub fn with_revision(mut self, rev: &str) -> Self {
        self.last_seen_revision = Some(rev.to_owned());
        self
    }

    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }
}

/// Outcome of [`fetch_season`]: fetched vs resume-skipped file names plus
/// the `meta-revised` stamps observed (feed them back as
/// `last_seen_revision`, via [`recrawl_hint`], on the next pass).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FetchReport {
    pub fetched: Vec<String>,
    pub skipped: Vec<String>,
    pub revisions: Vec<PageRevision>,
}

/// Reject path components that could escape the caller-supplied directory
/// (`..`, separators, empty). `raw_snapshot_path` joins literals, so
/// validated components keep every write under `out_dir`.
fn check_component(what: &str, s: &str) -> Result<(), FetchError> {
    if s.is_empty()
        || s == "."
        || s == ".."
        || s.contains('/')
        || s.contains('\\')
        || s.contains('\0')
    {
        return Err(FetchError::UnsafePath(format!("{what}={s:?}")));
    }
    Ok(())
}

/// BR per-page freshness stamp (e.g. `16:31:52 03-Sep-2026`) from
/// `<meta name="revised" content="…">` (`lastmod`/`last-modified` accepted
/// too). Opaque string; `None` when the page carries no stamp — then
/// [`recrawl_hint`] never fires for it.
pub fn parse_meta_revised(html: &str) -> Option<String> {
    let mut pos = 0usize;
    while pos < html.len() {
        let rest = html.get(pos..)?;
        let rel = rest.find("<meta")?;
        let lt = pos + rel;
        if !is_tag_open(html, lt, b"meta") {
            pos = lt + 1;
            continue;
        }
        let end = tag_end(html, lt)?;
        let tag = html.get(lt..end)?;
        pos = end;
        let name = attr_value(tag, "name").unwrap_or_default().to_lowercase();
        match name.as_str() {
            "revised" | "lastmod" | "last-modified" | "modified" => {
                if let Some(c) = attr_value(tag, "content") {
                    let c = c.trim();
                    if !c.is_empty() {
                        return Some(c.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// CRC-32 (ISO 3309) table, built at compile time for the snapshot codec.
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC32_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// Valid gzip bytes (magic `1f 8b` included) for `raw`, using stored
/// (uncompressed) deflate blocks — one per 64 KiB. Readable by any gzip
/// tool; needs no compression dependency.
pub fn gzip_encode(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 32);
    out.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03]);
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let chunks = raw.chunks(65_535);
        let n = chunks.len();
        for (idx, chunk) in chunks.enumerate() {
            out.push(if idx + 1 == n { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out.extend_from_slice(&crc32(raw).to_le_bytes());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out
}

/// Inverse of [`gzip_encode`]: stored-block gzip streams only (what this
/// pipeline writes, plus `gzip -0`-style files). Anything else — bad magic,
/// flags, Huffman blocks, truncated or CRC-mismatched bytes — is `Err`,
/// never a panic.
pub fn gzip_decode(gz: &[u8]) -> Result<Vec<u8>, FetchError> {
    let bad = |msg: &str| FetchError::Decode(msg.to_owned());
    let header = gz
        .get(..10)
        .ok_or_else(|| bad("too short for gzip header"))?;
    if header[0] != 0x1F || header[1] != 0x8B {
        return Err(bad("bad gzip magic"));
    }
    if header[2] != 0x08 {
        return Err(bad("unsupported compression method"));
    }
    if header[3] != 0x00 {
        return Err(bad("unsupported gzip flags"));
    }
    let mut pos = 10usize;
    let mut out = Vec::new();
    loop {
        let b = *gz.get(pos).ok_or_else(|| bad("truncated deflate block"))?;
        pos += 1;
        if b & 0xF8 != 0 {
            return Err(bad("reserved deflate bits set"));
        }
        if b >> 1 & 0x03 != 0 {
            return Err(bad("only stored deflate blocks are supported"));
        }
        let last = b & 0x01 == 1;
        let at = |o: usize| {
            gz.get(pos + o)
                .copied()
                .ok_or_else(|| bad("truncated block length"))
        };
        let len = u16::from_le_bytes([at(0)?, at(1)?]) as usize;
        let nlen = u16::from_le_bytes([at(2)?, at(3)?]);
        if len as u16 != !nlen {
            return Err(bad("block length mismatch"));
        }
        pos += 4;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| bad("block length overflow"))?;
        let data = gz
            .get(pos..end)
            .ok_or_else(|| bad("truncated block data"))?;
        out.extend_from_slice(data);
        pos = end;
        if last {
            break;
        }
    }
    let trailer = |o: usize| -> Result<u32, FetchError> {
        Ok(u32::from_le_bytes([
            *gz.get(pos + o)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 1)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 2)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
            *gz.get(pos + o + 3)
                .ok_or_else(|| bad("truncated gzip trailer"))?,
        ]))
    };
    let want_crc = trailer(0)?;
    let want_len = trailer(4)?;
    if out.len() as u32 != want_len {
        return Err(bad("length mismatch"));
    }
    if crc32(&out) != want_crc {
        return Err(bad("crc32 mismatch"));
    }
    Ok(out)
}

/// Gzip `html` into `path`, creating parent dirs. `path` must already be
/// resolved under the caller dir (see [`fetch_season`]); this helper only
/// writes where told.
pub fn write_snapshot_gz(path: &Path, html: &str) -> Result<(), FetchError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(FetchError::Io)?;
        }
    }
    std::fs::write(path, gzip_encode(html.as_bytes())).map_err(FetchError::Io)?;
    Ok(())
}

/// Inverse of [`write_snapshot_gz`].
pub fn read_snapshot_gz(path: &Path) -> Result<String, FetchError> {
    let bytes = std::fs::read(path).map_err(FetchError::Io)?;
    let raw = gzip_decode(&bytes)?;
    String::from_utf8(raw).map_err(|e| FetchError::Decode(format!("snapshot is not UTF-8: {e}")))
}

/// Fetch one season batch: every job in order, `>= FETCH_MIN_INTERVAL`
/// apart, each body stored as gzip at
/// `out_dir.join(raw_snapshot_path(source, season, file_name))`. Files
/// already on disk are skipped without touching the network (resume); pass
/// [`FetchJob::force`] to re-fetch one. Nothing is ever written outside
/// `out_dir`: path components are validated before joining. Returns which
/// files were fetched versus resume-skipped, plus the observed
/// `meta-revised` stamps (feed each stamp to [`recrawl_hint`]).
pub fn fetch_season<C: FetchClient>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
) -> Result<FetchReport, FetchError> {
    fetch_season_with_sleeper(client, source, season, jobs, out_dir, &mut |d| {
        std::thread::sleep(d);
    })
}

/// [`fetch_season`] with injectable sleep, so tests can observe etiquette
/// waits without waiting them. Paces at [`FETCH_MIN_INTERVAL`]; see
/// [`fetch_season_with_sleeper_at`] for a caller-chosen interval.
pub fn fetch_season_with_sleeper<C, S>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
    sleep: &mut S,
) -> Result<FetchReport, FetchError>
where
    C: FetchClient,
    S: FnMut(Duration),
{
    fetch_season_with_sleeper_at(
        client,
        source,
        season,
        jobs,
        out_dir,
        FETCH_MIN_INTERVAL,
        sleep,
    )
}

/// [`fetch_season_with_sleeper`] with a caller-chosen minimum spacing
/// between requests. `interval` below [`FETCH_MIN_INTERVAL`] exceeds BR's
/// robots.txt Crawl-delay 3 — the caller owns that decision (the
/// `nbatv-crawl` bin exposes it as `--interval`).
pub fn fetch_season_with_sleeper_at<C, S>(
    client: &C,
    source: &str,
    season: &str,
    jobs: &[FetchJob],
    out_dir: &Path,
    interval: Duration,
    sleep: &mut S,
) -> Result<FetchReport, FetchError>
where
    C: FetchClient,
    S: FnMut(Duration),
{
    check_component("source", source)?;
    check_component("season", season)?;
    let mut report = FetchReport::default();
    let mut last_fetch: Option<Instant> = None;
    for job in jobs {
        check_component("file_name", &job.file_name)?;
        let target = out_dir.join(raw_snapshot_path(source, season, &job.file_name));
        if !job.force && target.is_file() {
            report.skipped.push(job.file_name.clone());
            continue;
        }
        if let Some(t0) = last_fetch {
            let wait = interval.saturating_sub(t0.elapsed());
            if !wait.is_zero() {
                sleep(wait);
            }
        }
        let html = client.fetch(&job.url)?;
        last_fetch = Some(Instant::now());
        let revision = PageRevision {
            page: job.file_name.clone(),
            meta_revised: parse_meta_revised(&html),
        };
        write_snapshot_gz(&target, &html)?;
        report.fetched.push(job.file_name.clone());
        report.revisions.push(revision);
    }
    Ok(report)
}
