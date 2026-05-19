//! `dp-store-pg` — Postgres implementation of the `dp-domain::Store`
//! trait.
//!
//! Owns:
//!
//! * The `PgPool` handle (wrapped by `starter_store_postgres::Pool`).
//! * The SQL under `migrations/dp/`, exposed via [`sources()`] as a
//!   single [`MigrationSource`]. The host binary registers it (along
//!   with any other migration sources it cares about) and runs them
//!   through `starter_store_postgres::migrate`.
//! * Every SQL body that satisfies the `Store` trait.
//!
//! Boundary rule (TODO §0.6): the only allowed `starter_*` imports
//! here are `starter_spi::` and `starter_store_postgres::`.
//! `scripts/check-boundaries.sh` enforces this in CI.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod encode;
mod store;

pub use store::PgStore;

use starter_store_postgres::MigrationSource;

/// The static sqlx migrator built from the SQL files under
/// `migrations/dp/`. Embedded into the binary at compile time so the
/// crate doesn't need filesystem access at runtime.
static DP_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/dp");

/// Every migration source this crate contributes. Returned as a
/// `Vec` (length 1 today, but the shape leaves room for namespaced
/// follow-on sources without breaking callers) so the host binary
/// can do
///
/// ```ignore
/// use starter_store_postgres::migrate;
///
/// let mut m = migrate(&pool);
/// for s in dp_store_pg::sources() {
///     m = m.with_source(s);
/// }
/// m.run().await?;
/// ```
///
/// The source name `"dp"` matches the directory and the
/// `_sqlx_migrations_dp` progress table the runner creates.
pub fn sources() -> Vec<MigrationSource> {
    vec![MigrationSource {
        name: "dp",
        migrator: &DP_MIGRATOR,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_exposes_dp_migration() {
        let s = sources();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "dp");
        // Guard the assumption that the migrator embedded at least
        // the v1 init migration. If migrations/dp/ is ever emptied
        // by accident, this catches it.
        assert!(s[0].migrator.iter().count() >= 1);
    }
}
