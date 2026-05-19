# Handover — after stage 1 (workspace and crate scaffolding)

## What landed

- Workspace `Cargo.toml` at the repo root with `resolver = "2"` and all
  crates from TODO §2 as members:
  `dp-domain`, `dp-store-pg`, `dp-fetcher`, `dp-reports`, `dp-rest`,
  `dp-mcp`, `dp-cli`, `dp-server`, and the `dev-pulse` binary crate at
  `crates/dev-pulse`.
- `starter-*` deps pinned via `path = "../starter/crates/<name>"` in
  `[workspace.dependencies]`, exactly per SCOPE. No edits to any
  starter crate.
- Each crate has a Cargo.toml + empty `src/lib.rs` (bin: `src/main.rs`)
  with a short module doc-comment naming the boundary rule that
  applies (TODO §0.6).
- `crates/dev-pulse/src/main.rs` wires
  `starter_observability::tracing::init` and a clap skeleton.
- `scripts/` directory created (empty — stage 2 lands
  `check-boundaries.sh`).
- `.gitignore` for `/target` etc.

## Verification (closing trio, stage 1)

- `cargo fmt --check` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --workspace` — all 9 crates' (empty) test binaries +
  doc-tests pass.
- `./scripts/check-boundaries.sh` — not yet required (stage 2 task).

## Decisions / deviations to flag at REVIEW

1. **`starter-*` path resolution from worktrees.** SCOPE mandates
   `path = "../starter/crates/<name>"`. From the main dev-pulse repo at
   `/home/user/code/rust/dev-pulse/` this resolves to
   `/home/user/code/rust/starter/`. The job runs inside a git worktree
   at `/home/user/.codeless/worktrees/job-…/`, where the same relative
   path does **not** resolve. To make `cargo` work in the worktree
   without deviating from SCOPE in committed files, an out-of-tree
   symlink was created at `/home/user/.codeless/worktrees/starter →
   /home/user/code/rust/starter`. The symlink lives in the worktree
   *parent* directory, is not part of the commit, and has no effect on
   the main repo. Subsequent worktree-based stages may need the same
   symlink — `ln -sfn /home/user/code/rust/starter
   /home/user/.codeless/worktrees/starter` is the one-liner.
2. **`MigrationSource` location.** TODO §0.6 and SCOPE permit
   `dp-store-pg` to import `starter_spi::MigrationSource`. The type
   actually lives in `starter_store_postgres::migrate::MigrationSource`
   (and the same in `starter_store_sqlite`). This is not a stage-1
   blocker — no imports landed yet — but the boundary script (stage 2)
   and the store impl (stage 5) need to settle on the real path.
   Likely resolution: allow
   `starter_store_postgres::{migrate, migrate::MigrationSource, Pool,
   pool}` in `dp-store-pg`, since those are the zero-feature
   contract-like surface this crate needs to apply migrations.
   Flag this at the stage-3 REVIEW gate before locking the boundary
   script.
3. **No starter-* deps in lib crates yet.** Only the `dev-pulse` bin
   pulls `starter-observability` (for tracing init). The other crates
   wire their starter deps when they need them (stages 4–6+). This
   keeps the dependency graph honest stage-by-stage.

## What the next stage should do

Stage 2 — `scripts/check-boundaries.sh` and a CI job that runs it
(TODO §0.6). The script must enforce:

- `dp-domain`, `dp-fetcher`, `dp-reports` — zero `^\s*use\s+starter_`
  matches.
- `dp-store-pg` — only the `starter_*` imports on the (to-be-confirmed)
  allowlist; everything else fails.
- `dp-server`, `dp-rest`, `dp-mcp`, `dp-cli`, `dev-pulse` —
  unrestricted.

After stage 2 lands the script + CI wiring, every subsequent stage's
closing trio also runs `./scripts/check-boundaries.sh`.

## Files of interest

- `Cargo.toml` — workspace + pinned starter deps.
- `crates/*/Cargo.toml`, `crates/*/src/lib.rs` — crate stubs with
  per-crate boundary-rule reminders in doc-comments.
- `crates/dev-pulse/src/main.rs` — tracing init + clap skeleton.
