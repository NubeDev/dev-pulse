//! [`AppState`] — the per-process state every dp-rest handler holds.
//!
//! Phase 4 stage 3 wired the [`Store`] handle (every report
//! handler reads through it). Later Phase 4 stages widen this struct.
//! SCOPE-PROJECTS stage 8 adds [`GitHubAppConfig`] — the
//! deployment-shaped flag bundle behind which the §13.6 App
//! permission bump (`issues: write`) and the §8.4 "writes not
//! available" affordance live.
//!
//! The struct is `Clone` (cheap — every field is an `Arc`) so the
//! axum `Router::with_state(...)` extractor pattern works without
//! per-request allocation.

use std::sync::Arc;

use dp_domain::store::Store;

use crate::app_permissions::GitHubAppConfig;

/// Application state shared across every dp-rest handler.
#[derive(Clone)]
pub struct AppState {
    /// Persistence handle. Reports read; admin handlers (later
    /// stages) will write.
    pub store: Arc<dyn Store>,
    /// GitHub App-side configuration. Carries the
    /// `github.app.request_issues_write` `dp-config` flag (§13.6)
    /// and is consulted both by the §8.4 write-gate and by the
    /// `GET /me/app-install-banner` handler. Held as `Arc` for
    /// cheap clone across handlers.
    pub github_app: Arc<GitHubAppConfig>,
}

impl AppState {
    /// Convenience constructor — defaults `github_app` to
    /// [`GitHubAppConfig::default`] (the same defaults a
    /// freshly-shipped `dp-config` would produce).
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            github_app: Arc::new(GitHubAppConfig::default()),
        }
    }

    /// Override the GitHub App config. Used by the bin layer and
    /// by tests that need to exercise the `request_issues_write
    /// = false` branch without rebuilding the whole state.
    pub fn with_github_app(mut self, github_app: Arc<GitHubAppConfig>) -> Self {
        self.github_app = github_app;
        self
    }
}
