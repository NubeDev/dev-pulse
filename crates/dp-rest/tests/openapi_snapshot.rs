//! Snapshot test pinning the generated OpenAPI document to
//! `tests/openapi.snapshot.json`.
//!
//! Drift surfaces as a failing test — accidental schema changes
//! never silently break Phase 5 MCP / Phase 7 frontend clients
//! that consume this document. Regenerate the snapshot with:
//!
//! ```text
//! cargo test -p dp-rest -- --update-openapi-snapshot
//! ```
//!
//! The `--update-openapi-snapshot` flag is an environment-style
//! marker (we look for it in `std::env::args()`); Cargo passes
//! anything after `--` straight through to the test binary, so
//! this works without a custom test harness.

use std::fs;
use std::path::PathBuf;

use dp_rest::DevPulseApi;
use utoipa::OpenApi;

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("openapi.snapshot.json")
}

fn current_document() -> String {
    let doc = DevPulseApi::openapi();
    // `to_pretty_json` already returns canonical 2-space JSON; pin
    // a trailing newline so the file ends cleanly on Unix.
    let mut s = doc
        .to_pretty_json()
        .expect("serialise DevPulseApi to pretty JSON");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn update_requested() -> bool {
    std::env::args().any(|a| a == "--update-openapi-snapshot")
        || std::env::var("UPDATE_OPENAPI_SNAPSHOT").is_ok()
}

#[test]
fn openapi_snapshot_matches() {
    let current = current_document();
    let path = snapshot_path();

    if update_requested() || !path.exists() {
        fs::write(&path, &current)
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        // Treat update-runs as a pass so CI users can opt into the
        // regen on a single invocation without a second compile.
        return;
    }

    let stored = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    if stored != current {
        // Print a short hint plus the first diverging line so the
        // failure is actionable without a separate diff tool.
        let mismatch_hint = first_diff_line(&stored, &current);
        panic!(
            "openapi.snapshot.json is out of date.\n\
             Run `cargo test -p dp-rest -- --update-openapi-snapshot` \
             to regenerate it.\n\
             {mismatch_hint}",
        );
    }
}

fn first_diff_line(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!(
                "First diff at line {}:\n  stored:  {la}\n  current: {lb}",
                i + 1
            );
        }
    }
    match a.lines().count().cmp(&b.lines().count()) {
        std::cmp::Ordering::Less => {
            format!("Stored doc is shorter ({} lines) than current.", a.lines().count())
        }
        std::cmp::Ordering::Greater => {
            format!("Stored doc is longer ({} lines) than current.", a.lines().count())
        }
        std::cmp::Ordering::Equal => "Lines match — divergence is in trailing bytes.".to_string(),
    }
}
