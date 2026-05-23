-- Per-section "N/A" flags for the project executive summary.
--
-- Some projects legitimately have no hardware (firmware-only), no
-- commercial story (internal tool), or no documents yet. Forcing
-- every section to be filled in to reach the §3.4 submit threshold
-- punishes those projects with busy-work or an inflated change log.
--
-- `skipped_sections` is the simplest shape that works: a free-form
-- text array of section ids. The completion calc OR's the skip flag
-- alongside the existing per-section rule, so a skipped section
-- counts as "complete" for the % bar and the submit gate without
-- needing the user to dummy-up content.
--
-- We deliberately don't constrain the values via CHECK — the
-- application layer owns the closed set (see `EXEC_SUMMARY_SECTIONS`
-- on the frontend) and any unknown id is just ignored by the
-- completion calc. Keeps future section additions a one-line patch
-- rather than a schema migration.

ALTER TABLE dp_project_exec_summary
  ADD COLUMN skipped_sections text[] NOT NULL DEFAULT '{}';
