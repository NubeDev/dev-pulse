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
use starter_spi::blob::BlobStore;

use crate::app_permissions::GitHubAppConfig;
use crate::board_links::{
    OrgProjectsPickerBackend, UnconfiguredOrgProjectsPicker,
};
use crate::issue_dates::{ProjectV2MirrorBackend, UnconfiguredProjectV2Mirror};
use crate::issues_write::{IssueWriteBackend, UnconfiguredIssueWriter};
use crate::project_milestones::{MilestoneWriteBackend, UnconfiguredMilestoneWriter};

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
    /// GitHub I/O backend for the milestone write surface
    /// (`POST /projects/{id}/milestones`). Mirrors the
    /// [`issue_writer`][AppState::issue_writer] pattern: the
    /// handler calls into this trait after the
    /// [`crate::app_permissions::require_issues_write`] gate; on
    /// success the returned GitHub payload is parsed and upserted
    /// into `dp_milestones` so the local row reflects the write
    /// before the next reconciler tick.
    ///
    /// The default — [`UnconfiguredMilestoneWriter`] — refuses
    /// every call (`upstream_unavailable`) so deployments that
    /// haven't wired a real backend fail loudly. Wire one via
    /// [`AppState::with_milestone_writer`] from the bin layer.
    pub milestone_writer: Arc<dyn MilestoneWriteBackend>,
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
    /// Org-scoped Projects v2 picker backend used by
    /// `GET /orgs/{org_id}/projects-v2` (linear-projects-v2.md
    /// §7.3) — the §6.4 link-a-board dialog reads from this. The
    /// default — [`UnconfiguredOrgProjectsPicker`] — returns the
    /// `upstream_unavailable` 400 so the dialog can render an
    /// `[Open GitHub project settings]` hint instead of a blank
    /// dropdown. Bin layer wires
    /// [`crate::board_links::OctocrabOrgProjectsPicker`] when a
    /// GitHub token / install is armed.
    pub org_projects_picker: Arc<dyn OrgProjectsPickerBackend>,
    /// OAuth identity store handle. The `/me/identities` surface
    /// (§3.0 / §10) reads through it to project the viewer's
    /// linked third-party identities for the Account → Identities
    /// page. `None` in test builds and any composition root that
    /// has not yet wired the starter-auth-oauth `IdentityStore`;
    /// the handler degrades to a 503 in that case so a
    /// misconfigured deployment fails loudly instead of silently
    /// returning an empty list.
    pub identity_store: Option<Arc<dyn IdentityStore>>,
    /// Blob storage backend for the project Executive Summary
    /// image / document upload + proxy routes
    /// ([`crate::project_exec_summary`]). `None` in test builds and
    /// any composition root that has not wired a [`BlobStore`]; in
    /// that case the upload handlers return `503
    /// blob_storage_unavailable` and the proxy returns `404`.
    /// Production binaries wire a `MemoryBlobStore` (dev) or
    /// `FsBlobStore` / `GarageBlobStore` (prod) via
    /// [`AppState::with_blob_store`].
    pub blob_store: Option<Arc<dyn BlobStore>>,
    /// External-facing base URL (mirrors `server.base_url`), used to
    /// compose the QR payload `{base_url}/u/{unit_id}?t=<token>` for
    /// the Product & Manufacturing unit landing (P2 §6). `None` in
    /// test builds; the unit QR / landing degrade gracefully.
    pub public_base_url: Option<String>,
    /// HMAC secret for the token-gated public unit landing route
    /// (`MANUFACTURING_QR_SECRET`, P2 §6). The QR token is
    /// `HMAC-SHA256(secret, unit_id)`. `None` ⇒ no valid token can be
    /// minted, so `/u/{id}` 404s and `qr.svg` reports unavailable.
    pub manufacturing_qr_secret: Option<Arc<String>>,
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
            milestone_writer: Arc::new(UnconfiguredMilestoneWriter),
            scheduler: None,
            projectv2_mirror: Arc::new(UnconfiguredProjectV2Mirror),
            org_projects_picker: Arc::new(UnconfiguredOrgProjectsPicker),
            identity_store: None,
            blob_store: None,
            public_base_url: None,
            manufacturing_qr_secret: None,
        }
    }

    /// Wire the external-facing base URL used to compose QR payloads
    /// for the Product & Manufacturing unit landing (§6).
    pub fn with_public_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.public_base_url = Some(base_url.into());
        self
    }

    /// Wire the HMAC secret for the token-gated public unit landing
    /// route (`MANUFACTURING_QR_SECRET`, §6).
    pub fn with_manufacturing_qr_secret(mut self, secret: impl Into<String>) -> Self {
        self.manufacturing_qr_secret = Some(Arc::new(secret.into()));
        self
    }

    /// Wire the [`BlobStore`] backing the project Executive Summary
    /// image / document upload + proxy routes. Bin layer constructs
    /// the engine (typically `MemoryBlobStore` in dev, `FsBlobStore`
    /// on a single-node deploy, `GarageBlobStore` in prod) and hands
    /// it in.
    pub fn with_blob_store(mut self, blob_store: Arc<dyn BlobStore>) -> Self {
        self.blob_store = Some(blob_store);
        self
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

    /// Override the org-scoped Projects v2 picker backend used by
    /// `GET /orgs/{org_id}/projects-v2`. Bin layer wires this with
    /// the shared fetcher client; tests can leave it unset and the
    /// route returns the `upstream_unavailable` 400.
    pub fn with_org_projects_picker(
        mut self,
        picker: Arc<dyn OrgProjectsPickerBackend>,
    ) -> Self {
        self.org_projects_picker = picker;
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

    /// Override the milestone-write backend. Bin layer wires this
    /// with the octocrab-backed implementation; tests pass a fake.
    pub fn with_milestone_writer(
        mut self,
        writer: Arc<dyn MilestoneWriteBackend>,
    ) -> Self {
        self.milestone_writer = writer;
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
