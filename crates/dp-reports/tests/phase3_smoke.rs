//! Phase 3 smoke tests (TODO §Phase 3 stage 9 — the CI merge gate).
//!
//! Each `#[test]` below maps 1:1 to a bullet on the Phase 3 stage-9
//! checklist. They are deliberately small, named after the property
//! they pin, and exercise the public `dp_reports::*` surface end-to-
//! end (no fakes-of-fakes) so a regression that breaks a downstream
//! surface (REST / MCP / frontend) trips here first.
//!
//! Map of tests → checklist:
//!
//! * `resolved_window_echoes_back_with_anchor_preserved`
//! * `three_lens_numbers_correct_on_co_author_fixture`
//! * `percentile_cont_returns_none_when_sample_under_five`
//! * `percentiles_match_expected_on_recorded_fixture`
//! * `data_as_of_per_org_and_combined_match_fetch_runs`
//! * `boundary_check_still_green`
//! * `no_means_anywhere`

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::store::EventActorRow;
use dp_domain::window::WindowAnchor;
use dp_reports::lenses::{all_orgs_combined, per_org_split, single_org};
use dp_reports::{
    compute_percentiles, count_by_user, filter_rows_for_metric, pick_freshness_headline,
    resolve_window_at, CountMetric, DataAsOf, DataAsOfExt, ScopeMode, WindowLabel, WindowSpec,
    MIN_PERCENTILE_SAMPLE_N,
};

// ---------------------------------------------------------------------------
// resolved-window-echoes-back-with-anchor-preserved (TODO §0.4)
// ---------------------------------------------------------------------------

#[test]
fn resolved_window_echoes_back_with_anchor_preserved() {
    // Pick a deterministic "now" in mid-week so last_week resolves to
    // the Mon→Mon range without straddling a year boundary.
    let now = Utc.with_ymd_and_hms(2025, 5, 14, 12, 0, 0).single().unwrap();

    for anchor in [WindowAnchor::Viewer, WindowAnchor::Org, WindowAnchor::Utc] {
        let spec = WindowSpec {
            label: WindowLabel::LastWeek,
            tz: "Australia/Sydney".into(),
            anchor,
            custom_start: None,
            custom_end: None,
        };
        let w = resolve_window_at(&spec, now).expect("last_week resolves");

        // Resolved window must echo label, tz and anchor verbatim.
        assert_eq!(w.label, "last_week", "label echoed verbatim ({:?})", anchor);
        assert_eq!(w.tz, "Australia/Sydney", "tz echoed verbatim ({:?})", anchor);
        assert_eq!(w.anchor, anchor, "anchor preserved through resolution");

        // And start/end must be concrete UTC instants with start < end.
        assert!(w.start < w.end, "resolved window non-empty ({:?})", anchor);
    }
}

// ---------------------------------------------------------------------------
// three-lens-numbers-correct-on-co-author-fixture (SCOPE §8.1 + §0.2)
// ---------------------------------------------------------------------------

#[test]
fn three_lens_numbers_correct_on_co_author_fixture() {
    // U1 + U2 co-author a single commit visible in two orgs; U2 also
    // has an unrelated org-B commit.
    let event_shared = Uuid::new_v4();
    let event_org_b_only = Uuid::new_v4();
    let u1 = Uuid::new_v4();
    let u2 = Uuid::new_v4();
    let org_a = Uuid::new_v4();
    let org_b = Uuid::new_v4();
    let repo_a = Uuid::new_v4();
    let repo_b = Uuid::new_v4();
    let ts = Utc.with_ymd_and_hms(2025, 1, 6, 10, 0, 0).single().unwrap();

    let mk = |event_id, user_id, role, org_id, repo_id, kind, ts| EventActorRow {
        event_id,
        user_id,
        role,
        org_id,
        repo_id,
        kind,
        ts,
    };

    let rows = vec![
        mk(event_shared, u1, ActorRole::Author, org_a, repo_a, EventKind::Commit, ts),
        mk(event_shared, u1, ActorRole::Author, org_b, repo_b, EventKind::Commit, ts),
        mk(event_shared, u2, ActorRole::CoAuthor, org_a, repo_a, EventKind::Commit, ts),
        mk(event_shared, u2, ActorRole::CoAuthor, org_b, repo_b, EventKind::Commit, ts),
        mk(
            event_org_b_only,
            u2,
            ActorRole::Author,
            org_b,
            repo_b,
            EventKind::Commit,
            ts + chrono::Duration::hours(25),
        ),
    ];

    let filtered = filter_rows_for_metric(&rows, CountMetric::CommitsAuthored, None);

    // single_org A: each contributor credited once for the shared commit.
    let lens_a = single_org(&filtered, org_a);
    let counts_a = count_by_user(&lens_a);
    assert_eq!(counts_a.get(&u1).copied(), Some(1), "single_org A: U1 = 1");
    assert_eq!(counts_a.get(&u2).copied(), Some(1), "single_org A: U2 = 1");
    assert_eq!(lens_a.len(), 2, "single_org A total = 2");

    // single_org B: shared + extra commit by U2.
    let lens_b = single_org(&filtered, org_b);
    let counts_b = count_by_user(&lens_b);
    assert_eq!(counts_b.get(&u1).copied(), Some(1), "single_org B: U1 = 1");
    assert_eq!(counts_b.get(&u2).copied(), Some(2), "single_org B: U2 = 2");
    assert_eq!(lens_b.len(), 3, "single_org B total = 3");

    // all_orgs_combined: de-dup on (user_id, event_id), so U1=1 U2=2.
    let combined = all_orgs_combined(&filtered);
    let counts_combined = count_by_user(&combined);
    assert_eq!(counts_combined.get(&u1).copied(), Some(1), "combined: U1 = 1");
    assert_eq!(counts_combined.get(&u2).copied(), Some(2), "combined: U2 = 2");
    assert_eq!(combined.len(), 3, "combined total = 3 (de-dup applied)");

    // per_org_split: (user, org) buckets distinct from the other two.
    let split = per_org_split(&filtered);
    let actual: BTreeMap<(Uuid, Uuid), usize> =
        split.iter().map(|(k, v)| (*k, v.len())).collect();
    assert_eq!(actual.get(&(u1, org_a)).copied(), Some(1));
    assert_eq!(actual.get(&(u1, org_b)).copied(), Some(1));
    assert_eq!(actual.get(&(u2, org_a)).copied(), Some(1));
    assert_eq!(actual.get(&(u2, org_b)).copied(), Some(2));

    // Sanity: the three lens views are distinct shapes — single_org B
    // sees 3, single_org A sees 2, combined sees 3 (but with different
    // per-user distribution than B), per_org_split has 4 buckets.
    assert_ne!(lens_a.len(), lens_b.len(), "single_org A vs B differ");
    assert_eq!(actual.len(), 4, "per_org_split distinguishes (user, org)");
}

// ---------------------------------------------------------------------------
// percentile_cont-returns-none-when-sample-under-five (SCOPE §15.9)
// ---------------------------------------------------------------------------

#[test]
fn percentile_cont_returns_none_when_sample_under_five() {
    for n in 0..MIN_PERCENTILE_SAMPLE_N {
        let sample: Vec<i64> = (0..n as i64).map(|i| 100 * (i + 1)).collect();
        let p = compute_percentiles(&sample);
        assert!(p.p50.is_none(), "n={}: p50 must be None", n);
        assert!(p.p90.is_none(), "n={}: p90 must be None", n);
        assert!(p.p95.is_none(), "n={}: p95 must be None", n);
        assert_eq!(p.sample_n, n as u64, "n={}: sample_n echoed", n);
    }

    // And at n=5 the percentiles are populated.
    let p5 = compute_percentiles(&[10, 20, 30, 40, 50]);
    assert!(p5.p50.is_some(), "n=5: p50 populated");
    assert!(p5.p90.is_some(), "n=5: p90 populated");
    assert!(p5.p95.is_some(), "n=5: p95 populated");
    assert_eq!(p5.sample_n, 5);
}

// ---------------------------------------------------------------------------
// percentiles-match-expected-on-recorded-fixture
// ---------------------------------------------------------------------------

#[test]
fn percentiles_match_expected_on_recorded_fixture() {
    // Recorded review-turnaround durations (seconds). The expected
    // p50/p90/p95 values below match Postgres `percentile_cont` for
    // this exact sample (verified against
    // SELECT percentile_cont(ARRAY[0.5,0.9,0.95])
    //   WITHIN GROUP (ORDER BY d) FROM unnest(...) d;
    let durations: Vec<i64> = vec![
        300,    //  5 min
        600,    // 10 min
        1_200,  // 20 min
        1_800,  // 30 min
        3_600,  //  1 h
        7_200,  //  2 h
        14_400, //  4 h
        28_800, //  8 h
        43_200, // 12 h
        86_400, // 24 h
    ];
    let p = compute_percentiles(&durations);
    assert_eq!(p.sample_n, durations.len() as u64);

    // p50: between 3_600 and 7_200, rank = 0.5*(10-1) = 4.5
    //   → 3_600 + 0.5*(7_200-3_600) = 5_400.
    let p50 = p.p50.expect("p50 populated");
    assert!((p50 - 5_400.0).abs() < 1e-6, "p50={}, expected 5400", p50);

    // p90: rank = 0.9*9 = 8.1 → 43_200 + 0.1*(86_400-43_200) = 47_520.
    let p90 = p.p90.expect("p90 populated");
    assert!((p90 - 47_520.0).abs() < 1e-6, "p90={}, expected 47520", p90);

    // p95: rank = 0.95*9 = 8.55 → 43_200 + 0.55*(86_400-43_200) = 66_960.
    let p95 = p.p95.expect("p95 populated");
    assert!((p95 - 66_960.0).abs() < 1e-6, "p95={}, expected 66960", p95);
}

// ---------------------------------------------------------------------------
// data_as_of-per-org-and-combined-match-fetch_runs
// ---------------------------------------------------------------------------

#[test]
fn data_as_of_per_org_and_combined_match_fetch_runs() {
    // Simulate the store's projection of `fetch_runs` + per-org
    // cursor max(updated_at): the webhook + reconciler headlines
    // come from the latest finished run per kind, and per_org is the
    // max cursor advance per org.
    let webhook_latest = Utc.with_ymd_and_hms(2025, 5, 19, 10, 0, 0).single().unwrap();
    let reconciler_latest = Utc.with_ymd_and_hms(2025, 5, 19, 8, 0, 0).single().unwrap();

    let org_a = Uuid::new_v4();
    let org_b = Uuid::new_v4();
    let org_c = Uuid::new_v4();
    let ts_a = Utc.with_ymd_and_hms(2025, 5, 19, 9, 0, 0).single().unwrap();
    let ts_b = Utc.with_ymd_and_hms(2025, 5, 19, 7, 0, 0).single().unwrap();
    // org_c has no cursor row yet — absent from per_org.

    let d = DataAsOf {
        webhook_latest: Some(webhook_latest),
        reconciler_latest: Some(reconciler_latest),
        per_org: HashMap::from([(org_a, ts_a), (org_b, ts_b)]),
    };

    // Headlines straight from fetch_runs.
    assert_eq!(d.webhook_latest, Some(webhook_latest));
    assert_eq!(d.reconciler_latest, Some(reconciler_latest));

    // Per-org lens: hit returns the cursor tick, miss returns None.
    assert_eq!(d.for_single_org(org_a), Some(ts_a));
    assert_eq!(d.for_single_org(org_b), Some(ts_b));
    assert_eq!(d.for_single_org(org_c), None);

    // Combined lens: min across the requested orgs that are present.
    assert_eq!(
        d.for_all_orgs_combined(&[org_a, org_b]),
        Some(ts_b),
        "combined = min(per_org) across requested orgs"
    );
    // Absent org doesn't drag the headline to None — only present
    // entries participate in the min.
    assert_eq!(d.for_all_orgs_combined(&[org_a, org_c]), Some(ts_a));
    // No orgs requested → None.
    assert_eq!(d.for_all_orgs_combined(&[]), None);
    // All requested orgs absent → None (pending first reconcile).
    assert_eq!(d.for_all_orgs_combined(&[org_c]), None);

    // Headline picker matches the manual lens picks.
    assert_eq!(
        pick_freshness_headline(&d, ScopeMode::SingleOrg, &[org_a]),
        Some(ts_a)
    );
    assert_eq!(
        pick_freshness_headline(&d, ScopeMode::AllOrgsCombined, &[org_a, org_b]),
        Some(ts_b)
    );
    assert_eq!(
        pick_freshness_headline(&d, ScopeMode::PerOrgSplit, &[org_a, org_b]),
        None,
        "per_org_split has no single headline"
    );
}

// ---------------------------------------------------------------------------
// boundary-check-still-green (scripts/check-boundaries.sh)
// ---------------------------------------------------------------------------

#[test]
fn boundary_check_still_green() {
    let repo_root = repo_root();
    let script = repo_root.join("scripts").join("check-boundaries.sh");
    assert!(
        script.is_file(),
        "scripts/check-boundaries.sh not found at {}",
        script.display()
    );

    let output = Command::new("bash")
        .arg(&script)
        .current_dir(&repo_root)
        .output()
        .expect("invoke scripts/check-boundaries.sh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "boundary check failed (zero starter_* imports expected in dp-reports)\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout,
        stderr,
    );
}

// ---------------------------------------------------------------------------
// no-means-anywhere (grep guard)
// ---------------------------------------------------------------------------

#[test]
fn no_means_anywhere() {
    // Equivalent to:
    //   grep -rn "avg\|mean" crates/dp-reports/src \
    //     | grep -v "// not used"
    // …filtered to code (non-comment) lines so that "no means
    // anywhere" doc-comments don't trip the guard. Any hit on metric
    // code (i.e. a non-comment line containing `avg` or `mean`) is a
    // SCOPE §6 violation: this layer percentiles, it does not average.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits: Vec<String> = Vec::new();
    walk_rs(&src, &mut |path, line_no, line| {
        let trimmed = line.trim_start();
        // Skip line-comments and block-comment continuations (// or *).
        if trimmed.starts_with("//") || trimmed.starts_with("*") {
            return;
        }
        // Skip an explicit "not used" exemption (the grep filter on
        // the checklist).
        if line.contains("// not used") {
            return;
        }
        if contains_word(line, "avg") || contains_word(line, "mean") {
            hits.push(format!("{}:{}: {}", path.display(), line_no, line));
        }
    });
    assert!(
        hits.is_empty(),
        "metric code must not reference avg/mean (SCOPE §6 — no means anywhere):\n{}",
        hits.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    // tests run with CARGO_MANIFEST_DIR = crates/dp-reports.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .expect("repo root above crates/dp-reports")
        .to_path_buf()
}

fn walk_rs(dir: &Path, visit: &mut dyn FnMut(&Path, usize, &str)) {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, visit);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for (i, line) in text.lines().enumerate() {
                visit(&path, i + 1, line);
            }
        }
    }
}

/// Whole-word match — `mean` matches but `means`, `meaning`, `meant`
/// don't. Avoids false-positives on natural English doc text while
/// still catching identifier references.
fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let wbytes = word.as_bytes();
    let mut i = 0;
    while i + wbytes.len() <= bytes.len() {
        if &bytes[i..i + wbytes.len()] == wbytes {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_idx = i + wbytes.len();
            let after_ok = after_idx == bytes.len() || !is_ident_byte(bytes[after_idx]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
