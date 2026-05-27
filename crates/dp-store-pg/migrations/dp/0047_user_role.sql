-- Operator-controlled role tier on dp_users (DOCS/SCOPE-AUTHZ-USERS.md §2.1).
--
-- Three tiers (reader < writer < admin). New rows default to
-- `reader` so freshly-OAuth'd users land on the least-privilege
-- tier; the org-gate in policy/dev-pulse.toml still applies on top.
--
-- Backfill policy: every existing row stays at the default
-- `reader`. We do NOT auto-promote the CLI-seeded break-glass
-- admin row here because the canonical signal (which user was
-- seeded by `dev-pulse create-admin`) lives outside dp_users — it
-- lands in the starter-auth-users tables. The break-glass path is
-- restored by the operator running `dev-pulse set-role <email> admin`
-- once after deploy; the CLI subcommand ships in the same change.

ALTER TABLE dp_users
    ADD COLUMN role TEXT NOT NULL DEFAULT 'reader'
        CHECK (role IN ('reader', 'writer', 'admin'));
