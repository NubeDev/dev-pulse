//! Spot-check fixture harness — SCOPE §11.4 trust gate, the merge gate
//! for Phase 3.
//!
//! Each fixture under `tests/fixtures/*.json` pairs a recorded
//! GitHub-shaped activity payload (projected into the
//! [`EventActorRow`] shape the Phase 1 store hands the report layer)
//! with one or more `checks` that pin the expected output of the
//! report pipeline: filter-for-metric → apply scope lens → group by
//! user (and optionally by `(user, org)` for the `per_org_split`
//! lens).
//!
//! The pipeline these tests run is **the same pipeline a real report
//! request takes** — no shortcuts, no fakes-of-fakes. Every helper
//! used here is the public `dp_reports::*` surface:
//!
//! * [`dp_reports::filter_rows_for_metric`]
//! * [`dp_reports::single_org`] / [`dp_reports::all_orgs_combined`]
//!   / [`dp_reports::per_org_split`]
//! * [`dp_reports::count_by_user`]
//!
//! If a future change to any of those silently shifts a count, the
//! matching fixture's `expected_total` / `expected_by_user` /
//! `expected_by_user_org` will diverge and the harness will fail with
//! a fixture-named assertion message. That is the merge gate.
//!
//! ## Fixture format
//!
//! ```json
//! {
//!   "name": "...",
//!   "description": "...",
//!   "rows": [{"event_id":"…","user_id":"…","role":"author",
//!             "org_id":"…","repo_id":"…","kind":"commit",
//!             "ts":"2025-01-06T10:00:00Z"}, …],
//!   "checks": [
//!     { "name":"…",
//!       "scope_mode":"single_org|all_orgs_combined|per_org_split",
//!       "scope_org_id":"…" /* required only for single_org */,
//!       "metric":"commits_authored|…",   // CountMetric, snake_case
//!       "expected_total": 6,             // optional
//!       "expected_by_user": {"…uuid…": 6}, // optional
//!       "expected_by_user_org": {"…uuid…|…uuid…": 3} // per_org_split only
//!     }
//!   ]
//! }
//! ```
//!
//! ## Recorded payload provenance
//!
//! The `rows` array is a projection of recorded GitHub REST responses
//! through the Phase 1 ingestion path (one `activity_events` row per
//! source event, fanned out across `event_actors`). Storing the raw
//! GitHub JSON alongside would bloat the repo; the `rows` form is the
//! reproducible artefact the report pipeline actually consumes, and is
//! the one the SCOPE §11.4 reference table cross-checks against. When a
//! new fixture is added with a fresh raw payload, drop the raw JSON
//! under `tests/fixtures/raw/` and reference it from the fixture's
//! `description` field (see also `github_recorded_payload_ref` in the
//! existing files).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use dp_domain::event::{ActorRole, EventKind};
use dp_domain::store::EventActorRow;
use dp_reports::lenses::{all_orgs_combined, per_org_split, single_org};
use dp_reports::{count_by_user, filter_rows_for_metric, CountMetric};

// ---------------------------------------------------------------------------
// Fixture deserialisation types. These are test-only — the production
// EventActorRow does not implement Serialize/Deserialize because it is
// the store-projection row, not a wire type.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Fixture {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    description: String,
    #[serde(default)]
    #[allow(dead_code)]
    github_recorded_payload_ref: Option<String>,
    rows: Vec<RowSpec>,
    checks: Vec<Check>,
}

#[derive(Debug, Deserialize)]
struct RowSpec {
    event_id: Uuid,
    user_id: Uuid,
    role: ActorRole,
    org_id: Uuid,
    repo_id: Uuid,
    kind: EventKind,
    ts: DateTime<Utc>,
}

impl RowSpec {
    fn into_row(self) -> EventActorRow {
        EventActorRow {
            event_id: self.event_id,
            user_id: self.user_id,
            role: self.role,
            org_id: self.org_id,
            repo_id: self.repo_id,
            kind: self.kind,
            ts: self.ts,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Check {
    name: String,
    scope_mode: ScopeModeTag,
    #[serde(default)]
    scope_org_id: Option<Uuid>,
    metric: CountMetric,
    #[serde(default)]
    expected_total: Option<u64>,
    #[serde(default)]
    expected_by_user: Option<BTreeMap<Uuid, u64>>,
    #[serde(default)]
    expected_by_user_org: Option<BTreeMap<String, u64>>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ScopeModeTag {
    SingleOrg,
    AllOrgsCombined,
    PerOrgSplit,
}

// ---------------------------------------------------------------------------
// Pipeline runner — the exact path every Phase 3 report takes.
// ---------------------------------------------------------------------------

fn run_check(fixture_name: &str, check: &Check, rows: &[EventActorRow]) {
    // Step 1: filter to the metric (kind + default actor_roles).
    let filtered = filter_rows_for_metric(rows, check.metric, None);

    // Step 2 + 3: apply the scope lens and reduce.
    match check.scope_mode {
        ScopeModeTag::SingleOrg => {
            let org_id = check.scope_org_id.unwrap_or_else(|| {
                panic!(
                    "[{}::{}] single_org check missing scope_org_id",
                    fixture_name, check.name
                )
            });
            let lensed = single_org(&filtered, org_id);
            assert_total_and_by_user(fixture_name, check, &lensed);
        }
        ScopeModeTag::AllOrgsCombined => {
            let lensed = all_orgs_combined(&filtered);
            assert_total_and_by_user(fixture_name, check, &lensed);
        }
        ScopeModeTag::PerOrgSplit => {
            let buckets = per_org_split(&filtered);
            if let Some(expected) = &check.expected_by_user_org {
                let actual: BTreeMap<String, u64> = buckets
                    .iter()
                    .map(|((u, o), v)| (format!("{}|{}", u, o), v.len() as u64))
                    .collect();
                assert_eq!(
                    &actual, expected,
                    "[{}::{}] per_org_split bucket counts differ",
                    fixture_name, check.name
                );
            } else {
                panic!(
                    "[{}::{}] per_org_split check must set expected_by_user_org",
                    fixture_name, check.name
                );
            }
        }
    }
}

fn assert_total_and_by_user(fixture_name: &str, check: &Check, rows: &[EventActorRow]) {
    if let Some(expected_total) = check.expected_total {
        assert_eq!(
            rows.len() as u64,
            expected_total,
            "[{}::{}] total row count after lens differs",
            fixture_name,
            check.name,
        );
    }
    if let Some(expected_by_user) = &check.expected_by_user {
        let actual = count_by_user(rows);
        assert_eq!(
            &actual, expected_by_user,
            "[{}::{}] per-user count differs",
            fixture_name, check.name,
        );
    }
}

// ---------------------------------------------------------------------------
// Fixture loader.
// ---------------------------------------------------------------------------

fn load_fixture(path: &Path) -> Fixture {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read fixture {}: {}", path.display(), e));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse fixture {}: {}", path.display(), e))
}

fn run_fixture(filename: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(filename);
    let fixture = load_fixture(&path);

    let rows: Vec<EventActorRow> = fixture.rows.into_iter().map(RowSpec::into_row).collect();
    assert!(
        !fixture.checks.is_empty(),
        "fixture {} has no checks — at least one is required",
        filename
    );
    for check in &fixture.checks {
        run_check(&fixture.name, check, &rows);
    }
}

// ---------------------------------------------------------------------------
// One #[test] per fixture so a failure points at the responsible file.
// ---------------------------------------------------------------------------

#[test]
fn spot_check_single_user_single_org() {
    run_fixture("single-user-single-org.json");
}

#[test]
fn spot_check_co_authored_commit_spanning_two_orgs() {
    run_fixture("co-authored-commit-spanning-two-orgs.json");
}

#[test]
fn spot_check_home_org_split_on_shared_org() {
    run_fixture("home-org-split-on-shared-org.json");
}
