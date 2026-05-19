## Done

- Wrote `scripts/check-boundaries.sh` (executable) enforcing TODO.md §0.6: zero `starter_*` imports in `dp-domain`/`dp-fetcher`/`dp-reports`, and `dp-store-pg` may only import `starter_spi::*`. Uses `git grep -nE` so it respects `.gitignore` and works in CI and locally.
- Verified positive (clean tree → `check-boundaries: OK`, exit 0) and negative (synthetic scratch repo with violations → exit 1, prints offending file:line for each rule).
- Wired CI: `.github/workflows/boundaries.yml` runs the script on push to `main` and on every PR.
- Closing trio green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `./scripts/check-boundaries.sh` all pass.
- Committed as `stage 2: boundary enforcement — scripts/check-boundaries.sh and CI job` on branch `codeless/bootstrap-domain-store` (1287d58). Not pushed — no remote auth in this session.

## Next

- Stage 3 is a REVIEW gate per WORKFLOW.md — do not advance. Summarise stages 1+2, surface the deferred open questions below, wait for approval.

## What you need to know

- `dp-store-pg` allowlist is the entire `starter_spi::` crate, matching TODO §0.6's "MigrationSource + zero-dep contract types" phrasing. Tightening is a one-line script edit if desired.
- Boundary script greps with `^[[:space:]]*(pub[[:space:]]+)?use[[:space:]]+starter_`, so docstrings/comments mentioning starter crates do not trip it.
- Worktree symlink still required: `ln -sfn /home/user/code/rust/starter /home/user/.codeless/worktrees/starter` (carried from stage 1). Cargo cannot resolve `../starter/crates/*` from the worktree without it.

## Open questions

- (deferred to stage 3 REVIEW) Keep allowlist as `starter_spi::*`, or expand to `starter_store_postgres::{migrate, Pool, pool}` so stage 5 can actually compile? `MigrationSource` lives in `starter_store_postgres::migrate`, not `starter_spi`, despite TODO §0.6's wording.
- (deferred to stage 3 REVIEW) Replace the out-of-tree worktree symlink with absolute paths / `[patch]` so a fresh worktree boots without manual setup?
