## Done

- Cargo workspace scaffolded at repo root with all nine crates from TODO §2 (dp-domain, dp-store-pg, dp-fetcher, dp-reports, dp-rest, dp-mcp, dp-cli, dp-server, dev-pulse bin).
- Each crate has an empty lib.rs (or main.rs for the bin) with a doc-comment naming the §0.6 boundary rule that applies.
- starter-* deps pinned via `path = "../starter/crates/<name>"` in `[workspace.dependencies]`, no starter edits.
- `crates/dev-pulse/src/main.rs` wires `starter_observability::tracing::init` + a clap skeleton.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all green.
- Committed as `stage 1: workspace and crate scaffolding` on branch `codeless/bootstrap-domain-store`. Not pushed (no remote auth in this session).
- Handover written at `.codeless/jobs/bootstrap-domain-store/handover.md`.

## Next

- Stage 2: write `scripts/check-boundaries.sh` per TODO §0.6 and wire a CI job that runs it. Closing trio of every subsequent stage adds `./scripts/check-boundaries.sh` to the verify list.
- Stage 3 is the first REVIEW gate — confirm the deviations flagged below and the `starter_*` allowlist for `dp-store-pg` before stage 4 starts.

## What you need to know

- **Worktree path quirk.** SCOPE mandates `path = "../starter/crates/<name>"`. That resolves correctly from the main repo at `/home/user/code/rust/dev-pulse/` (sibling of `/home/user/code/rust/starter/`) but not from the worktree at `/home/user/.codeless/worktrees/job-…/`. To make cargo work in the worktree without polluting committed files, an out-of-tree symlink was created: `ln -sfn /home/user/code/rust/starter /home/user/.codeless/worktrees/starter`. Any future stage that opens a fresh worktree needs that symlink (it's not part of the commit). One-liner is in handover.md.
- **MigrationSource lives in `starter_store_postgres::migrate::MigrationSource`, not `starter_spi`** (SCOPE/TODO §0.6 phrasing is slightly off). No imports landed in stage 1, so no decision needed yet — but the stage-2 boundary script and stage-5 store impl have to agree on the real allowlist. Flagged in handover.md for the stage-3 REVIEW gate.
- Only the `dev-pulse` bin pulls a starter crate so far (`starter-observability`). The other crates wire their starter deps stage-by-stage as they need them, to keep the dep graph honest.
- Cargo.lock is committed.

## Open questions

- (deferred to stage 3 REVIEW gate, per WORKFLOW.md) Confirm the `dp-store-pg` starter-import allowlist for the boundary script: literally `starter_spi::MigrationSource` (which does not exist) vs. the real `starter_store_postgres::{migrate, migrate::MigrationSource, Pool, pool}` surface.
- (deferred to stage 3 REVIEW gate) Worktree-symlink approach OK, or should we switch to absolute paths / a different mechanism so a fresh worktree boots without manual setup?
