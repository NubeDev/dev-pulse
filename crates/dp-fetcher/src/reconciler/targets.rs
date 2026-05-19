//! [`RepoTarget`] + the pluggable [`TargetProvider`] trait the
//! reconciler enumerates work over.
//!
//! The reconciler does not enumerate repos by reading the [`Store`]
//! directly because the exact "which repos belong to which org"
//! mapping is the responsibility of the bin layer (it already
//! consults `starter-config` for the GitHub App installation list
//! and any operator filters). Decoupling here also makes the
//! reconciler trivially testable — tests inject a hand-rolled
//! provider returning a fixed set of [`RepoTarget`]s instead of
//! standing up a Postgres + an Octocrab session graph.
//!
//! Per TODO §0.3, cursors are keyed `(org_id, repo_id,
//! resource_kind)`. A [`RepoTarget`] is the value side of the
//! `(org_id, repo_id)` half — we also carry the GitHub-side
//! identifiers (`owner_login`, `repo_name`, plus their numeric
//! `github_id`s) because the reconciler synthesises webhook-shaped
//! payloads to feed through the same handler path the webhook
//! worker uses (Stage 5), and the handler `upsert_*` helpers key on
//! `github_id` not on our internal UUID.
//!
//! [`Store`]: dp_domain::Store

use async_trait::async_trait;
use dp_domain::StoreError;
use uuid::Uuid;

/// One repo the reconciler should tick over.
///
/// Cheap to clone; lifetimes are short (just the tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoTarget {
    /// Internal org id.
    pub org_id: Uuid,
    /// GitHub numeric id for the org / owner.
    pub org_github_id: i64,
    /// GitHub login (`octocat`, `nube-io`, …). Used as the
    /// `{owner}` path segment in GitHub API calls.
    pub owner_login: String,
    /// Internal repo id.
    pub repo_id: Uuid,
    /// GitHub numeric id for the repo.
    pub repo_github_id: i64,
    /// GitHub repo name (the `{repo}` path segment).
    pub repo_name: String,
}

/// Source of truth for "which repos do we tick over".
///
/// Implementations can read from the [`Store`], a static config
/// list, or a hard-coded slice in tests. The reconciler holds an
/// `Arc<dyn TargetProvider>` so the bin layer can swap the
/// implementation without recompiling `dp-fetcher`.
///
/// [`Store`]: dp_domain::Store
#[async_trait]
pub trait TargetProvider: Send + Sync {
    /// Return the full set of repos the reconciler may tick over.
    /// Filtering by [`super::Scope`] happens inside the reconciler
    /// itself — the provider is the unfiltered superset.
    async fn list_targets(&self) -> Result<Vec<RepoTarget>, StoreError>;
}

/// Simple in-memory provider — used by tests and as a sensible
/// default when the bin layer wants to ship a static repo list
/// without writing its own trait impl.
pub struct StaticTargets {
    targets: Vec<RepoTarget>,
}

impl StaticTargets {
    /// Build a provider over the given fixed list.
    pub fn new(targets: Vec<RepoTarget>) -> Self {
        Self { targets }
    }
}

#[async_trait]
impl TargetProvider for StaticTargets {
    async fn list_targets(&self) -> Result<Vec<RepoTarget>, StoreError> {
        Ok(self.targets.clone())
    }
}
