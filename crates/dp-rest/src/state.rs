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
use dp_fetcher::reconciler::Scheduler;
use starter_auth_oauth::IdentityStore;

use crate::app_permissions::GitHubAppConfig;
use crate::issue_dates::{ProjectV2MirrorBackend, UnconfiguredProjectV2Mirror};
use crate::issues_write::{IssueWriteBackend, UnconfiguredIssueWriter};
use crate::repo_project_link::{
    ProjectsPickerBackend, UnconfiguredProjectsPicker,
};

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
    /// GitHub I/O backend for the §8 issue write surface. The
    /// per-verb handlers (POST `/issues`, PATCH `/issues/{id}`,
    /// POST `/issues/{id}/comments`) call into this trait between
    /// the §8.2 step 5 CAS and the step 7 commit. Held as an `Arc`
    /// so cloning the state is cheap.
    ///
    /// The default — [`UnconfiguredIssueWriter`] — refuses every
    /// call with a `Server { status: 503 }` error so deployments
    /// that have not wired a real backend fail loudly instead of
    /// silently bypassing GitHub. Wire a production backend via
    /// [`AppState::with_issue_writer`] from the bin layer.
    pub issue_writer: Arc<dyn IssueWriteBackend>,
    /// Reconciler scheduler — used by `POST /repos/{id}/sync` to
    /// hand-trigger a per-repo reconciler tick. `None` in test
    /// builds and the §5.9 handler degrades to "queued: false" /
    /// 503 in that case.
    pub scheduler: Option<Arc<Scheduler>>,
    /// Projects v2 GraphQL mirror backend used by
    /// `PATCH /issues/{id}/dates` (§3.10). The default —
    /// [`UnconfiguredProjectV2Mirror`] — declines every call so
    /// deployments that have not wired a real mirror simply skip
    /// the best-effort enqueue / spawn entirely; the local
    /// upsert remains authoritative.
    pub projectv2_mirror: Arc<dyn ProjectV2MirrorBackend>,
    /// Projects v2 picker backend used by
    /// `GET /repos/{id}/projects` to surface a project + field
    /// chooser in the admin pane. The default —
    /// [`UnconfiguredProjectsPicker`] — returns a 503 so the
    /// UI can degrade to a paste-node-id text field when no
    /// GraphQL transport is wired.
    pub projects_picker: Arc<dyn ProjectsPickerBackend>,
    /// OAuth identity store handle. The `/me/identities` surface
    /// (§3.0 / §10) reads through it to project the viewer's
    /// linked third-party identities for the Account → Identities
    /// page. `None` in test builds and any composition root that
    /// has not yet wired the starter-auth-oauth `IdentityStore`;
    /// the handler degrades to a 503 in that case so a
    /// misconfigured deployment fails loudly instead of silently
    /// returning an empty list.
    pub identity_store: Option<Arc<dyn IdentityStore>>,
}

impl AppState {
    /// Convenience constructor — defaults `github_app` to
    /// [`GitHubAppConfig::default`] (the same defaults a
    /// freshly-shipped `dp-config` would produce).
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            github_app: Arc::new(GitHubAppConfig::default()),
            issue_writer: Arc::new(UnconfiguredIssueWriter),
            scheduler: None,
            projectv2_mirror: Arc::new(UnconfiguredProjectV2Mirror),
            projects_picker: Arc::new(UnconfiguredProjectsPicker),
            identity_store: None,
        }
    }

    /// Override the Projects v2 mirror backend. Bin layer wires
    /// this from the GraphQL transport; tests pass a fake.
    pub fn with_projectv2_mirror(
        mut self,
        mirror: Arc<dyn ProjectV2MirrorBackend>,
    ) -> Self {
        self.projectv2_mirror = mirror;
        self
    }

    /// Override the Projects v2 picker backend used by
    /// `GET /repos/{id}/projects`. Bin layer wires this with the
    /// shared fetcher client so the admin pane surfaces the
    /// project + field chooser; tests can leave it unset and the
    /// route returns 503.
    pub fn with_projects_picker(
        mut self,
        picker: Arc<dyn ProjectsPickerBackend>,
    ) -> Self {
        self.projects_picker = picker;
        self
    }

    /// Wire a reconciler scheduler so `POST /repos/{id}/sync` can
    /// hand-trigger a tick. Bin layer calls this; tests can leave
    /// it unset and the handler returns 503.
    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    /// Override the issue-write backend. Bin layer wires this with
    /// the octocrab-backed implementation; tests pass a fake.
    pub fn with_issue_writer(mut self, writer: Arc<dyn IssueWriteBackend>) -> Self {
        self.issue_writer = writer;
        self
    }

    /// Override the GitHub App config. Used by the bin layer and
    /// by tests that need to exercise the `request_issues_write
    /// = false` branch without rebuilding the whole state.
    pub fn with_github_app(mut self, github_app: Arc<GitHubAppConfig>) -> Self {
        self.github_app = github_app;
        self
    }

    /// Wire the OAuth identity store so `GET /me/identities`
    /// can project the viewer's linked third-party identities.
    /// The bin layer passes the same `Arc<dyn IdentityStore>` it
    /// hands to the OAuth router (so the two surfaces agree on
    /// what's linked); tests can leave it unset and the handler
    /// returns 503.
    pub fn with_identity_store(
        mut self,
        identity_store: Arc<dyn IdentityStore>,
    ) -> Self {
        self.identity_store = Some(identity_store);
        self
    }
}
