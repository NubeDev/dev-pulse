#!/usr/bin/env bash
# scripts/check-boundaries.sh
#
# Enforces the starter-* import boundary rule from TODO.md §0.6:
#
#   - dp-domain, dp-fetcher, dp-reports MUST NOT contain any
#     `starter_*` imports. Zero exceptions.
#   - dp-store-pg MAY import only from `starter_spi::` and
#     `starter_store_postgres::` (the Postgres pool + MigrationSource
#     runner the crate is built on). Any other `starter_*` import in
#     dp-store-pg is a CI failure.
#   - dp-server, dp-rest, dp-mcp, dp-cli, dev-pulse (bin) are
#     unrestricted — not checked.
#
# Exits 0 on clean, 1 on violation. Designed to run both locally
# and in CI; uses `git grep` so it respects .gitignore and works
# from any checkout.

set -euo pipefail

# Run from the repo root so the paths below resolve regardless of
# where the script is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# The forbidden-everywhere crates.
FORBIDDEN_CRATES=(
  "crates/dp-domain"
  "crates/dp-fetcher"
  "crates/dp-reports"
)

# Pattern: a `use starter_<something>` import line (allowing
# leading whitespace and `pub use`). We deliberately match on
# `use` rather than bare `starter_` so docstrings and comments
# that *mention* starter crates do not trip the check.
USE_RE='^[[:space:]]*(pub[[:space:]]+)?use[[:space:]]+starter_'

violations=0

# ---- 1. Zero starter_* imports in dp-domain / dp-fetcher / dp-reports.
for crate in "${FORBIDDEN_CRATES[@]}"; do
  if [[ ! -d "$crate" ]]; then
    echo "check-boundaries: missing crate directory: $crate" >&2
    violations=$((violations + 1))
    continue
  fi
  # `git grep -E` so we only scan tracked files; --quiet would hide
  # the offending lines, so capture output and print it on hit.
  if hits=$(git grep -nE "$USE_RE" -- "$crate" 2>/dev/null); then
    echo "check-boundaries: forbidden starter_* import in $crate:" >&2
    echo "$hits" >&2
    violations=$((violations + 1))
  fi
done

# ---- 2. dp-store-pg: only `starter_spi::` and
#        `starter_store_postgres::` are allowed.
STORE_PG="crates/dp-store-pg"
if [[ -d "$STORE_PG" ]]; then
  # Find every `use starter_*` line, then drop the ones that begin
  # with the allowed prefixes. Anything left is a violation.
  if all_uses=$(git grep -nE "$USE_RE" -- "$STORE_PG" 2>/dev/null); then
    bad=$(echo "$all_uses" \
      | grep -vE '^[^:]+:[0-9]+:[[:space:]]*(pub[[:space:]]+)?use[[:space:]]+starter_(spi|store_postgres)(::|;|[[:space:]])' \
      || true)
    if [[ -n "$bad" ]]; then
      echo "check-boundaries: $STORE_PG may only import starter_spi::* or starter_store_postgres::*; found:" >&2
      echo "$bad" >&2
      violations=$((violations + 1))
    fi
  fi
else
  echo "check-boundaries: missing crate directory: $STORE_PG" >&2
  violations=$((violations + 1))
fi

if (( violations > 0 )); then
  echo "check-boundaries: FAIL ($violations violation group(s))" >&2
  exit 1
fi

echo "check-boundaries: OK"
