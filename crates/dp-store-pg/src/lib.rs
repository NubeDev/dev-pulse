//! `dp-store-pg` — Postgres implementation of the `dp-domain::Store`
//! trait. Owns `PgPool` and the SQL migrations under `migrations/dp/`.
//!
//! Stage 1 scaffold: empty crate. The store impl, the `sources()`
//! function (returning `[starter_auth_users, dp]` per the starter
//! migrations namespacing rule), and the migration files land in
//! stages 5–6.
//!
//! Boundary rule (TODO §0.6): the only allowed starter import in
//! this crate is `starter_spi::MigrationSource` (plus zero-dep
//! contract types).
