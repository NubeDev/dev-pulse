# Handover — after stage 2 (boundary enforcement)

## Done

- Wrote `scripts/check-boundaries.sh` per TODO.md §0.6. It fails CI on any
  `^\s*(pub\s+)?use\s+starter_` line in `crates/dp-domain`,
  `crates/dp-fetcher`, or `crates/dp-reports`, and on any non-`starter_spi::`
  starter import in `crates/dp-store-pg`. Other crates
  (server/rest/mcp/cli/dev-pulse bin) are unrestricted, matching §0.6.
- Verified the script with both a clean tree (exit 0, `OK`) and a
  synthetic scratch repo containing one violation per rule (exit 1, prints
  offending file:line for each).
- Added a CI job at `.github/workflows/boundaries.yml` that runs the
  script on push to `main` and on every PR.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and `./scripts/check-boundaries.sh` all green.
- Committed as `stage 2: boundary enforcement` on branch
  `codeless/bootstrap-domain-store`. Not pushed (no remote auth in this
  session — same caveat as stage 1).

## Next

- Stage 3 is the **first REVIEW gate** per WORKFLOW.md. Do not advance.
  Write the gate handover summarising stages 1+2, surface the two
  deferred questions (below), and wait for user approval.

## What you need to know

- **Worktree symlink still required.** `starter-*` paths in the root
  `Cargo.toml` are `../starter/crates/<name>`. From this worktree at
  `/home/user/.codeless/worktrees/job-…/`, that only resolves because
  `ln -sfn /home/user/code/rust/starter /home/user/.codeless/worktrees/starter`
  was created out-of-tree by stage 1. A fresh worktree must run that
  one-liner before any cargo command.
- **Boundary script uses `git grep`**, so it only scans tracked files
  and respects `.gitignore`. That means a stray untracked `.rs` with a
  bad import will not fail CI locally until it's `git add`-ed —
  acceptable, because CI checks out the committed tree.
- **dp-store-pg allowlist is `starter_spi::*` literally**, not just
  `MigrationSource`. TODO §0.6 phrases the rule as
  "MigrationSource + starter_spi's zero-dep contract types," so the
  allowlist is the whole `starter_spi::` crate. If we ever want tighter
  than that, we change the script — for now it matches the spec.
- **MigrationSource location mismatch (carried from stage 1).** TODO
  §0.6 says `starter_spi::MigrationSource`, but in starter the type
  actually lives at `starter_store_postgres::migrate::MigrationSource`.
  No imports landed yet, so the boundary script enforces the *spec'd*
  rule. If stage 5 finds it has to import `starter_store_postgres::…`
  to get a usable `MigrationSource`, the script will fail — and that
  is the REVIEW-gate decision point.

## Open questions

- (REVIEW gate, deferred since stage 1) Is the `starter_spi::*`
  allowlist correct, or do we need to expand it to
  `starter_store_postgres::{migrate, Pool, pool}` so stage 5 can
  actually compile? Pick one before stage 5.
- (REVIEW gate, deferred since stage 1) Keep the out-of-tree
  `/home/user/.codeless/worktrees/starter` symlink as the worktree
  bootstrap, or switch to absolute paths / `[patch]` so a fresh
  worktree boots without manual setup?
