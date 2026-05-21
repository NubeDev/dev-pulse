-- Per-user settings — the open-ended key/value store behind the
-- frontend "Account → Settings" page.
--
-- Designed as a generic K/V so new settings ship as a frontend
-- form field + a pinned key constant in `dp-rest::settings`,
-- *not* as a schema migration. The first consumer is the
-- per-user GitHub PAT (`github.pat`) — additional keys
-- (theme preference, default org, notification opt-ins, …)
-- land without touching this table.
--
-- Columns:
--
--   * `user_id`    — owner. Pins the row to a single dp user.
--   * `key`        — dotted-namespace string (e.g. `github.pat`,
--                    `ui.theme`). The REST layer rejects keys not
--                    in its pinned catalogue so a typo can't
--                    silently grow the schema.
--   * `value`      — opaque TEXT. The REST layer interprets the
--                    bytes per key (string, JSON, base64).
--                    NEVER returned verbatim for `is_secret`
--                    rows — the GET handler redacts to
--                    `{ has_value: true }`.
--   * `is_secret`  — when true, the GET handlers redact `value`.
--                    Pinned per-key on the server side; the
--                    column is kept here as defence-in-depth so
--                    a future direct-DB consumer (CLI export)
--                    still sees the bit.
--   * `updated_at` — last write. Used by the UI to show
--                    "last edited" and to invalidate downstream
--                    caches that depend on the value.
--
-- TODO(future): at-rest encryption of `value` when `is_secret`.
-- v1 stores the value as plain TEXT inside Postgres — the
-- database is not exposed to end-users and the REST layer
-- never returns secret values, but a future change should
-- wrap secret values with the same age key
-- `starter-secrets-file` already loads for the webhook secret.

CREATE TABLE dp_user_settings (
    user_id     UUID         NOT NULL REFERENCES dp_users(id) ON DELETE CASCADE,
    key         TEXT         NOT NULL,
    value       TEXT         NOT NULL,
    is_secret   BOOLEAN      NOT NULL DEFAULT FALSE,
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);

-- List path (`GET /me/settings`) is `WHERE user_id = ? ORDER BY key`.
-- The PK already covers (user_id, key) so no extra index needed.
