//! In-file unit tests for the fetch pipeline + snapshot codec (moved
//! verbatim from the pre-split single-file lib.rs).

use super::super::fetch::*;
use crate::csv::{raw_snapshot_path, recrawl_hint, PageRevision};

#[cfg(test)]
mod pipeline {
    use super::*;

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// Canned-HTML client: no network, records call order.
    struct MapClient {
        pages: HashMap<String, String>,
        calls: RefCell<Vec<String>>,
    }

    impl MapClient {
        fn with(pages: &[(&str, &str)]) -> Self {
            MapClient {
                pages: pages
                    .iter()
                    .map(|(u, h)| ((*u).to_owned(), (*h).to_owned()))
                    .collect(),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl FetchClient for MapClient {
        fn fetch(&self, url: &str) -> Result<String, FetchError> {
            self.calls.borrow_mut().push(url.to_owned());
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::Client(format!("no fixture for {url}")))
        }
    }

    static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Unique caller-supplied dir under the system temp dir (std only, no
    /// `tempdir` crate). Removed best-effort when the test finishes.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn fresh(tag: &str) -> Self {
            let n = DIR_SEQ.fetch_add(1, Ordering::SeqCst);
            let mut path = std::env::temp_dir();
            path.push(format!("nbatv-ingest-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).expect("create temp dir");
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    const GAME_HTML: &str = "\
<html><head><meta name=\"revised\" content=\"09:00:00 04-Sep-2026\"></head>\
<body><div class=\"scorebox_meta\">November 1, 1946</div></body></html>";

    #[test]
    fn etiquette_constant_covers_crawl_delay_with_margin() {
        assert!(FETCH_MIN_INTERVAL >= Duration::from_millis(3_500));
        assert_eq!(
            etiquette_delay(Duration::from_millis(0)),
            FETCH_MIN_INTERVAL
        );
        assert_eq!(etiquette_delay(Duration::from_secs(10)), Duration::ZERO);
        assert_eq!(
            etiquette_delay(Duration::from_millis(1_000)),
            Duration::from_millis(2_500)
        );
    }

    #[test]
    fn gzip_round_trip_covers_empty_unicode_and_multiblock() {
        for raw in [
            Vec::new(),
            "<html>first game: NYK 68, TRH 66 — M\u{e4}ller</html>"
                .as_bytes()
                .to_vec(),
            vec![b'a'; 100_000],
        ] {
            let gz = gzip_encode(&raw);
            assert_eq!(&gz[0..2], &[0x1F, 0x8B]);
            assert_eq!(gzip_decode(&gz).unwrap(), raw);
        }
        // 100_000 bytes need two stored blocks (64 KiB cap each).
        assert!(gzip_encode(&vec![b'a'; 100_000]).len() > 65_535);
    }

    #[test]
    fn gzip_decode_rejects_garbage_without_panicking() {
        assert!(gzip_decode(b"").is_err());
        assert!(gzip_decode(b"not gzip at all................").is_err());
        let mut gz = gzip_encode(b"hello");
        assert!(gzip_decode(&gz[..gz.len() - 3]).is_err());
        gz[1] = 0x00;
        assert!(gzip_decode(&gz).is_err());
        let mut bad_crc = gzip_encode(b"hello");
        let n = bad_crc.len();
        bad_crc[n - 8] ^= 0xFF;
        assert!(gzip_decode(&bad_crc).is_err());
    }

    #[test]
    fn fetch_writes_gz_snapshot_and_records_revision() {
        let dir = TempDir::fresh("write");
        let client =
            MapClient::with(&[("https://br.example/boxscores/194611010TRH.html", GAME_HTML)]);
        let jobs = [FetchJob::new(
            "194611010TRH.html.gz",
            "https://br.example/boxscores/194611010TRH.html",
        )];
        let mut sleeps = Vec::new();
        let report = fetch_season_with_sleeper(
            &client,
            "br-box",
            "BAA_1947",
            &jobs,
            &dir.path,
            &mut |d: Duration| sleeps.push(d),
        )
        .unwrap();
        assert_eq!(report.fetched, vec!["194611010TRH.html.gz".to_owned()]);
        assert!(report.skipped.is_empty());
        // Single request: no etiquette wait needed.
        assert!(sleeps.is_empty());
        assert_eq!(client.calls.borrow().len(), 1);

        let target = dir.path.join(raw_snapshot_path(
            "br-box",
            "BAA_1947",
            "194611010TRH.html.gz",
        ));
        assert!(target.is_file());
        assert!(target.starts_with(&dir.path));
        assert_eq!(read_snapshot_gz(&target).unwrap(), GAME_HTML);
        assert_eq!(report.revisions.len(), 1);
        assert_eq!(report.revisions[0].page, "194611010TRH.html.gz");
        assert_eq!(
            report.revisions[0].meta_revised.as_deref(),
            Some("09:00:00 04-Sep-2026")
        );
        // The observed stamp feeds the existing freshness helper.
        assert!(recrawl_hint(None, &report.revisions[0]));
        assert!(!recrawl_hint(
            Some("09:00:00 04-Sep-2026"),
            &report.revisions[0]
        ));
    }

    #[test]
    fn fetch_skips_files_already_on_disk_without_network() {
        let dir = TempDir::fresh("resume");
        let target = dir.path.join(raw_snapshot_path(
            "br-box",
            "BAA_1947",
            "194611010TRH.html.gz",
        ));
        write_snapshot_gz(&target, GAME_HTML).unwrap();
        let before = std::fs::read(&target).unwrap();

        // Empty fixture map: any fetch attempt errors, so a network touch
        // would fail the run.
        let client = MapClient::with(&[]);
        let jobs = [FetchJob::new(
            "194611010TRH.html.gz",
            "https://br.example/boxscores/194611010TRH.html",
        )];
        let report = fetch_season(&client, "br-box", "BAA_1947", &jobs, &dir.path).unwrap();
        assert!(report.fetched.is_empty());
        assert_eq!(report.skipped, vec!["194611010TRH.html.gz".to_owned()]);
        assert!(client.calls.borrow().is_empty());
        assert_eq!(std::fs::read(&target).unwrap(), before);
    }

    #[test]
    fn fetch_paces_sequential_requests_with_etiquette_waits() {
        let dir = TempDir::fresh("pace");
        let client = MapClient::with(&[
            ("https://br.example/a.html", "<html>a</html>"),
            ("https://br.example/b.html", "<html>b</html>"),
        ]);
        let jobs = [
            FetchJob::new("a.html.gz", "https://br.example/a.html"),
            FetchJob::new("b.html.gz", "https://br.example/b.html"),
        ];
        let mut sleeps = Vec::new();
        let report = fetch_season_with_sleeper(
            &client,
            "br-box",
            "BAA_1947",
            &jobs,
            &dir.path,
            &mut |d: Duration| sleeps.push(d),
        )
        .unwrap();
        assert_eq!(report.fetched.len(), 2);
        // One season batch, in order, with one full etiquette wait between
        // the two requests (back-to-back fixture fetches take ~no time).
        assert_eq!(
            *client.calls.borrow(),
            vec![
                "https://br.example/a.html".to_owned(),
                "https://br.example/b.html".to_owned()
            ]
        );
        assert_eq!(sleeps.len(), 1);
        assert!(sleeps[0] >= Duration::from_secs(3));
        assert!(sleeps[0] <= FETCH_MIN_INTERVAL);
    }

    #[test]
    fn fetch_force_refetches_an_on_disk_snapshot() {
        let dir = TempDir::fresh("force");
        let target = dir
            .path
            .join(raw_snapshot_path("br-box", "BAA_1947", "a.html.gz"));
        write_snapshot_gz(&target, "<html>stale</html>").unwrap();
        let client = MapClient::with(&[("https://br.example/a.html", "<html>fresh</html>")]);
        let jobs = [FetchJob::new("a.html.gz", "https://br.example/a.html").force()];
        let report = fetch_season(&client, "br-box", "BAA_1947", &jobs, &dir.path).unwrap();
        assert_eq!(report.fetched, vec!["a.html.gz".to_owned()]);
        assert_eq!(read_snapshot_gz(&target).unwrap(), "<html>fresh</html>");
    }

    #[test]
    fn fetch_rejects_path_components_that_escape_the_caller_dir() {
        let dir = TempDir::fresh("unsafe");
        let client = MapClient::with(&[("https://br.example/a.html", "<html>a</html>")]);
        for (source, season, file) in [
            ("../outside", "BAA_1947", "a.html.gz"),
            ("br-box", "..", "a.html.gz"),
            ("br-box", "BAA_1947", "../evil.html.gz"),
            ("br-box", "BAA_1947", "sub/dir.html.gz"),
            ("br-box", "BAA_1947", ""),
        ] {
            let jobs = [FetchJob::new(file, "https://br.example/a.html")];
            assert!(
                matches!(
                    fetch_season(&client, source, season, &jobs, &dir.path),
                    Err(FetchError::UnsafePath(_))
                ),
                "must reject {source:?}/{season:?}/{file:?}"
            );
        }
        assert!(client.calls.borrow().is_empty());
    }

    #[test]
    fn meta_revised_parses_stamp_and_absent_means_no_signal() {
        assert_eq!(
            parse_meta_revised(GAME_HTML).as_deref(),
            Some("09:00:00 04-Sep-2026")
        );
        assert_eq!(
            parse_meta_revised("<html><body>no meta</body></html>"),
            None
        );
        let rev = PageRevision {
            page: "a.html.gz".to_owned(),
            meta_revised: None,
        };
        assert!(!recrawl_hint(None, &rev));
    }
}
