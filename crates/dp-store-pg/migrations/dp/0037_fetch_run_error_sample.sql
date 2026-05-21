-- ---------------------------------------------------------------------------
-- Capture per-run error samples so /admin/runs can explain *why* a run had
-- non-zero `errors`. Previously the only signal was the numeric `errors`
-- counter; the operator had to grep the structured log to find out what
-- actually failed.
--
-- Shape: JSONB array of objects, each carrying enough context to scan the
-- log without leaving the page:
--   [{"org": "octocat", "repo": "hello-world", "kind": "issues",
--     "error": "GitHub 502 Bad Gateway"}, ...]
--
-- Bounded by the writer (cap of ~10 entries, truncated strings) so a runaway
-- run can't bloat the row. Nullable — clean runs carry NULL, not `[]`, so
-- the column reads as a clear "no errors recorded".
-- ---------------------------------------------------------------------------

ALTER TABLE dp_fetch_runs
    ADD COLUMN error_sample JSONB NULL;
