## Done

- reviewed cumulative diff from stages 0–9 against Layer-1 invariants (R1 crate dep direction, R2 single transport, R4/R5 trust boundary, wire-format stability)
- confirmed dp-reports adds only pure types + SQL string builders; no upward crate deps; no transport changes; no edits to existing serde shapes

## Next

- stage 11 (promotion): land SCOPE.md §8.2 + §15.15 text, and reconcile against SCOPE.md line 49 ("UI will not render leaderboards") which currently contradicts the proposed UI surface
- still owed by the job goal but not yet started: REST + MCP + frontend wiring off `LeaderboardEnvelope` / `MyStandingEnvelope`

## What you need to know

- PASS verdict applies only to Layer-1 invariants — coverage of the job's "wire REST + MCP + frontend" deliverable is incomplete (zero refs to leaderboard/my_standing in `dp-rest`, `dp-mcp`, `dp-server`, or `frontend/`)
- `crates/dp-reports/src/lib.rs` re-exports the full public surface (envelopes, errors, bind orders, caps, defaults) so downstream wiring has a single import point
- `PageCursor` wire form is currently plain JSON (no base64 in workspace) — flagged in stage 6 handover as deliberate-but-opaque; changing it later is a breaking change for in-flight cursors

## Open questions

- how to reconcile SCOPE.md §line-49 prohibition on UI leaderboards with the proposed §8.2 promotion — must be resolved before stage 11 writes the SCOPE text
- whether the missing REST/MCP/frontend wiring should be added as additional WORK stages before promotion, or whether promotion of the types-only surface is acceptable

PASS: Layer-1 invariants hold — dp-reports stays a leaf crate with pure types + SQL builders, no transport added, no existing wire formats mutated, and the §6.9 trust boundary is enforced via a separate envelope/permission gate rather than a projection.
