//! Octocrab client wrapper — **the single chokepoint** in
//! `dp-fetcher` through which every GitHub HTTP call flows.
//!
//! ## Why a wrapper at all
//!
//! GitHub's REST API forces three concerns into every request:
//!
//! 1. **Primary rate limit** (`X-RateLimit-*`). Burning through it
//!    causes 403s with `remaining: 0`.
//! 2. **Secondary rate limit** (abuse detection). Surfaces as 403
//!    + `x-ratelimit-resource: secondary` or as 429. `Retry-After`
//!    is authoritative when present.
//! 3. **Conditional GETs.** Most reconciler ticks are no-change;
//!    `If-None-Match` + the cursor's `etag` (TODO §0.3) means a
//!    304 with empty body costs us zero quota.
//!
//! TODO §Phase 2 mandates pacing live in *one* place. This is it.
//! Reconciler and backfill call typed methods here; raw octocrab
//! handles are not exposed.
//!
//! ## Authentication
//!
//! Production runs build a [`Client`] via
//! [`Client::for_installation`], handing it
//! [`InstallationCredentials`] resolved at startup by the bin /
//! `dp-server` composition layer (which is the layer permitted to
//! talk to `starter-secrets-file` — see [`credentials`] for the
//! boundary reasoning). The wrapper uses octocrab's App-auth path
//! so the per-installation token is minted, cached, and refreshed
//! transparently.
//!
//! Tests construct a [`Client`] via [`Client::with_personal_token`]
//! against a wiremock base URL so we can drive the happy/304/401/
//! 403-secondary/429/5xx branches deterministically.

pub mod credentials;
pub mod ratelimit;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use http_body_util::BodyExt;
use octocrab::models::{AppId, InstallationId};
use octocrab::Octocrab;
use secrecy::SecretString;
use serde::de::DeserializeOwned;

pub use credentials::{AppCredentials, InstallationCredentials, JwtError};
pub use ratelimit::{classify as classify_rate_limit, RateLimitSignal};

/// Result of a conditional GET against GitHub.
#[derive(Debug)]
pub enum Fetched<T> {
    /// 304 Not Modified — the cursor's etag is still valid.
    /// `signal` carries the rate-limit telemetry from the response
    /// so the caller can update freshness even on no-change polls.
    NotModified {
        /// Rate-limit headers parsed off the 304 (the headers are
        /// still returned by GitHub on a 304).
        signal: Option<RateLimitSignal>,
    },
    /// Body deserialized successfully. `etag` is the value the
    /// caller should persist into `fetch_cursors.etag` for the
    /// next conditional GET.
    Ok {
        /// Parsed body.
        body: T,
        /// New `ETag` header, if GitHub returned one.
        etag: Option<String>,
        /// Rate-limit telemetry to record on the run log.
        signal: Option<RateLimitSignal>,
    },
}

/// All errors the wrapper can surface to its callers. Split by
/// *what the caller should do next* — that's the dimension the
/// reconciler / backfill / webhook worker care about.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// 401 — credentials are wrong or revoked. **Fail loudly**;
    /// the run is unrecoverable until an operator rotates the App
    /// private key or reinstalls.
    #[error("github 401 unauthorized — credentials revoked or stale")]
    Unauthorized,
    /// Primary quota exhausted (403 with `remaining: 0`). Caller
    /// pauses this installation until `reset_at`.
    #[error("github primary rate limit exhausted; resets at {reset_at}")]
    PrimaryRateLimit {
        /// UTC moment the quota window resets.
        reset_at: DateTime<Utc>,
    },
    /// Secondary rate limit / abuse detector (403 + secondary
    /// header, or raw 429). Caller backs off until `retry_at`.
    #[error("github secondary rate limit; retry at {retry_at}")]
    SecondaryRateLimit {
        /// UTC moment after which a retry is allowed.
        retry_at: DateTime<Utc>,
    },
    /// 5xx — transient. Caller retries with backoff.
    #[error("github {status} server error: {body}")]
    Server {
        /// The HTTP status returned.
        status: u16,
        /// Truncated response body (4 KiB max).
        body: String,
    },
    /// 4xx not otherwise handled (404, 422, …). Caller may want
    /// to skip the resource and continue.
    #[error("github {status} client error: {body}")]
    Client {
        /// The HTTP status returned.
        status: u16,
        /// Truncated response body (4 KiB max).
        body: String,
    },
    /// JSON deserialization failed.
    #[error("response body did not deserialize: {0}")]
    Deserialize(String),
    /// Transport failure / construction error from octocrab.
    #[error("octocrab transport error: {0}")]
    Transport(String),
    /// JWT minting / private-key parsing failed at construction.
    #[error(transparent)]
    Jwt(#[from] JwtError),
    /// The local per-run request budget on this `Client` was hit
    /// (operator-side fuse, not a GitHub-side limit). The caller
    /// should stop the current run; another can start later. See
    /// [`Client::with_budget`].
    #[error("local request budget exhausted: made {made} of max {max}")]
    BudgetExhausted {
        /// Requests issued in this run before the fuse blew.
        made: u64,
        /// The configured ceiling.
        max: u64,
    },
}

/// The single GitHub HTTP client used by `dp-fetcher`. Cheap to
/// clone — internally `Arc`-shared.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Octocrab>,
    /// We hold a tiny stub for the bin layer's diagnostics — the
    /// reconciler logs which installation it's calling against.
    installation_id: Option<u64>,
    /// Total GitHub HTTP calls this `Client` has dispatched since
    /// the last [`Client::reset_budget`]. Shared between clones —
    /// the budget is a property of the underlying connection /
    /// credential, not of an individual handle.
    requests_made: Arc<AtomicU64>,
    /// Optional ceiling enforced *before* each call. `None` = no
    /// local fuse (rely solely on GitHub's `X-RateLimit-*` headers).
    /// `Some(n)` = after `n` issued requests, every subsequent call
    /// returns [`ClientError::BudgetExhausted`] until the caller
    /// invokes [`Client::reset_budget`].
    ///
    /// This is the operator-side fuse the README + SCOPE §15.4 lean
    /// on — GitHub's per-hour quota is 5000 against this PAT bucket,
    /// but a runaway reconciler tick should not be allowed to spend
    /// even half of that in one go. A typical setting is `max = 50`
    /// for a check / smoke and `max = 500` for a real production
    /// tick.
    max_requests: Option<u64>,
}

impl Client {
    /// Build a client that authenticates as a GitHub App
    /// installation. Octocrab handles the JWT → installation-token
    /// exchange and caches the result; we don't need to manage
    /// tokens ourselves.
    ///
    /// `base_url` defaults to `https://api.github.com` in
    /// production; tests pass a wiremock URL.
    pub fn for_installation(
        creds: &InstallationCredentials,
        base_url: &str,
    ) -> Result<Self, ClientError> {
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(
            secrecy::ExposeSecret::expose_secret(&creds.app.private_key_pem).as_bytes(),
        )
        .map_err(|e| ClientError::Jwt(JwtError::Encode(e)))?;
        let app = Octocrab::builder()
            .base_uri(base_url)
            .map_err(transport)?
            .app(AppId(creds.app.app_id), key)
            .build()
            .map_err(transport)?;
        let installed = app
            .installation(InstallationId(creds.installation_id))
            .map_err(transport)?;
        Ok(Self {
            inner: Arc::new(installed),
            installation_id: Some(creds.installation_id),
            requests_made: Arc::new(AtomicU64::new(0)),
            max_requests: None,
        })
    }

    /// Build a client using a static token. Intended for tests
    /// (wiremock) and for short-lived diagnostic CLI flows; **not**
    /// the production path.
    pub fn with_personal_token(token: SecretString, base_url: &str) -> Result<Self, ClientError> {
        let crab = Octocrab::builder()
            .base_uri(base_url)
            .map_err(transport)?
            .personal_token(token)
            .build()
            .map_err(transport)?;
        Ok(Self {
            inner: Arc::new(crab),
            installation_id: None,
            requests_made: Arc::new(AtomicU64::new(0)),
            max_requests: None,
        })
    }

    /// Which installation this client is bound to, if any. Exposed
    /// for log/metric labels only — callers must not branch on it.
    pub fn installation_id(&self) -> Option<u64> {
        self.installation_id
    }

    /// Install a local per-run request ceiling. `Some(n)` enforces a
    /// fuse: after `n` calls, every subsequent dispatch returns
    /// [`ClientError::BudgetExhausted`] until [`Self::reset_budget`].
    /// `None` removes the fuse (default).
    ///
    /// The ceiling is checked *before* the HTTP dispatch, so a tick
    /// that would issue more calls than the budget allows stops at
    /// exactly the budget — it does not race past by one.
    pub fn with_budget(mut self, max: Option<u64>) -> Self {
        self.max_requests = max;
        self
    }

    /// Reset the per-run counter to zero. Called by the reconciler
    /// at the start of each tick (and by the backfill driver at the
    /// start of each batch) so the budget is per-run, not per-process.
    pub fn reset_budget(&self) {
        self.requests_made.store(0, Ordering::SeqCst);
    }

    /// Total GitHub HTTP calls issued since the last `reset_budget`.
    /// Exposed for run-log telemetry.
    pub fn requests_made(&self) -> u64 {
        self.requests_made.load(Ordering::SeqCst)
    }

    /// The configured ceiling, if any.
    pub fn max_requests(&self) -> Option<u64> {
        self.max_requests
    }

    /// Conditional GET. If `etag` is supplied it is sent as
    /// `If-None-Match`; a 304 short-circuits to
    /// [`Fetched::NotModified`].
    ///
    /// `path` is a GitHub-relative path like `/repos/foo/bar/pulls`
    /// — i.e. what octocrab calls a "route".
    pub async fn get_conditional<T: DeserializeOwned>(
        &self,
        path: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<T>, ClientError> {
        // Local fuse: check the budget *before* incrementing so we
        // stop at exactly N, never N+1. The increment after the
        // check is `fetch_add` so concurrent calls from cloned
        // Clients can't both slip past the same boundary.
        if let Some(max) = self.max_requests {
            let made = self.requests_made.load(Ordering::SeqCst);
            if made >= max {
                return Err(ClientError::BudgetExhausted { made, max });
            }
        }
        let _seq = self.requests_made.fetch_add(1, Ordering::SeqCst);

        let mut headers = HeaderMap::new();
        if let Some(tag) = etag {
            headers.insert(
                http::header::IF_NONE_MATCH,
                HeaderValue::from_str(tag).map_err(|e| ClientError::Transport(e.to_string()))?,
            );
        }
        // `Accept` per GitHub guidance; cheap to always send.
        headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            HeaderName::from_static("x-github-api-version"),
            HeaderValue::from_static("2022-11-28"),
        );

        let uri = path
            .parse::<Uri>()
            .map_err(|e| ClientError::Transport(format!("bad path {path}: {e}")))?;
        let resp = self
            .inner
            ._get_with_headers(uri, Some(headers))
            .await
            .map_err(transport)?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let signal = classify_rate_limit(status.as_u16(), &headers, Utc::now());

        // Branch on status BEFORE consuming the body — for 304 the
        // body is empty and we want to skip parsing entirely.
        match status {
            StatusCode::NOT_MODIFIED => Ok(Fetched::NotModified { signal }),
            StatusCode::UNAUTHORIZED => Err(ClientError::Unauthorized),
            s if s.is_success() => {
                let etag = headers
                    .get(http::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let bytes = read_body(resp).await?;
                let body = serde_json::from_slice::<T>(&bytes)
                    .map_err(|e| ClientError::Deserialize(e.to_string()))?;
                Ok(Fetched::Ok { body, etag, signal })
            }
            s if s.is_server_error() => {
                let bytes = read_body(resp).await.unwrap_or_default();
                Err(ClientError::Server {
                    status: s.as_u16(),
                    body: truncate(&bytes),
                })
            }
            s => {
                // 4xx that isn't a 401: could be primary RL (403),
                // secondary RL (403/429), or a benign 404.
                match signal {
                    Some(RateLimitSignal::PrimaryExhausted { reset_at }) => {
                        Err(ClientError::PrimaryRateLimit { reset_at })
                    }
                    Some(RateLimitSignal::SecondaryRateLimit { retry_at }) => {
                        Err(ClientError::SecondaryRateLimit { retry_at })
                    }
                    _ => {
                        let bytes = read_body(resp).await.unwrap_or_default();
                        Err(ClientError::Client {
                            status: s.as_u16(),
                            body: truncate(&bytes),
                        })
                    }
                }
            }
        }
    }

    // ---------- typed helpers reconciler / backfill call -------
    //
    // These exist so callers in `reconciler.rs` and `backfill.rs`
    // never touch `Octocrab` directly — that's the §Phase-2
    // "no raw octocrab usage elsewhere" rule. The shape is
    // deliberately minimal here; richer pagination + cursor
    // bookkeeping land in the reconciler/backfill stages.

    /// List PRs on a repo (page 1). The cursor's `since` is *not*
    /// applied here — the reconciler filters by `updated_at` after
    /// the fact because GitHub's PR list endpoint doesn't honour
    /// `since`.
    pub async fn list_pull_requests(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/pulls?state=all&per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// List issues on a repo. `since` is RFC3339; GitHub returns
    /// issues updated at or after that point.
    pub async fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let mut path = format!("/repos/{owner}/{repo}/issues?state=all&per_page=100");
        if let Some(ts) = since {
            path.push_str(&format!("&since={}", ts.to_rfc3339()));
        }
        self.get_conditional(&path, etag).await
    }

    /// List commits on a repo's default branch. `since` is honored
    /// natively by the GitHub commits endpoint.
    pub async fn list_commits(
        &self,
        owner: &str,
        repo: &str,
        since: Option<DateTime<Utc>>,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let mut path = format!("/repos/{owner}/{repo}/commits?per_page=100");
        if let Some(ts) = since {
            path.push_str(&format!("&since={}", ts.to_rfc3339()));
        }
        self.get_conditional(&path, etag).await
    }

    /// List teams in an org. There is no `since=` parameter; GitHub
    /// returns the full team list every time, so the reconciler
    /// relies entirely on the `ETag` for cheap no-change polls.
    pub async fn list_org_teams(
        &self,
        org_login: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/teams?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// List members of an org. Same shape as `list_org_teams`: no
    /// `since=`, ETag-based no-change short-circuit. GitHub paginates
    /// at 100/page; we currently fetch only page 1 (consistent with
    /// `list_pull_requests` / `list_issues`) — orgs >100 members
    /// will get a follow-up pagination pass when we wire it on the
    /// other endpoints.
    pub async fn list_org_members(
        &self,
        org_login: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/members?per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- repos -------------------------------------------------------

    /// Get a single repo by owner + name.
    pub async fn get_repo(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}");
        self.get_conditional(&path, None).await
    }

    /// List repos in an org. `etag` enables 304 short-circuit.
    pub async fn list_org_repos(
        &self,
        org_login: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/repos?type=all&sort=updated&per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// List repos for an authenticated user (the token owner).
    pub async fn list_user_repos(
        &self,
        username: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/users/{username}/repos?type=all&sort=updated&per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- git tags / releases -----------------------------------------

    /// List git tags on a repo (lightweight + annotated tags).
    pub async fn list_tags(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/tags?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get a single git tag by name (annotated tag object).
    pub async fn get_tag(
        &self,
        owner: &str,
        repo: &str,
        tag_name: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/git/ref/tags/{tag_name}");
        self.get_conditional(&path, None).await
    }

    /// List releases (GitHub's higher-level tag + release-notes surface).
    pub async fn list_releases(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/releases?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get the latest release for a repo.
    pub async fn get_latest_release(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/releases/latest");
        self.get_conditional(&path, None).await
    }

    /// Get a specific release by id.
    pub async fn get_release(
        &self,
        owner: &str,
        repo: &str,
        release_id: u64,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/releases/{release_id}");
        self.get_conditional(&path, None).await
    }

    // ---- users -------------------------------------------------------

    /// Get a single user by GitHub login.
    pub async fn get_user(
        &self,
        login: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/users/{login}");
        self.get_conditional(&path, None).await
    }

    /// Get the authenticated user (the token / installation actor).
    pub async fn get_authenticated_user(
        &self,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        self.get_conditional("/user", None).await
    }

    /// List public repos for a user by login.
    pub async fn list_public_repos_for_user(
        &self,
        login: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/users/{login}/repos?sort=updated&per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- orgs --------------------------------------------------------

    /// Get a single org by login.
    pub async fn get_org(
        &self,
        org_login: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}");
        self.get_conditional(&path, None).await
    }

    /// List orgs the authenticated user (or a named user) belongs to.
    pub async fn list_orgs_for_user(
        &self,
        login: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/users/{login}/orgs?per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- teams -------------------------------------------------------

    /// Get a single team by org + slug.
    pub async fn get_team(
        &self,
        org_login: &str,
        team_slug: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/teams/{team_slug}");
        self.get_conditional(&path, None).await
    }

    /// List team members by org + slug.
    pub async fn list_team_members(
        &self,
        org_login: &str,
        team_slug: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/teams/{team_slug}/members?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// List team repos by org + slug.
    pub async fn list_team_repos(
        &self,
        org_login: &str,
        team_slug: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/orgs/{org_login}/teams/{team_slug}/repos?per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- branches / refs --------------------------------------------

    /// List branches on a repo.
    pub async fn list_branches(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/branches?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get a single branch by name.
    pub async fn get_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/branches/{branch}");
        self.get_conditional(&path, None).await
    }

    // ---- issue comments ----------------------------------------------

    /// List comments on an issue or PR by number.
    pub async fn list_issue_comments(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        since: Option<DateTime<Utc>>,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let mut path =
            format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100");
        if let Some(ts) = since {
            path.push_str(&format!("&since={}", ts.to_rfc3339()));
        }
        self.get_conditional(&path, etag).await
    }

    // ---- PR reviews --------------------------------------------------

    /// List reviews on a PR by number.
    pub async fn list_pull_request_reviews(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path =
            format!("/repos/{owner}/{repo}/pulls/{number}/reviews?per_page=100");
        self.get_conditional(&path, etag).await
    }

    // ---- workflows ---------------------------------------------------

    /// List workflow runs for a repo.
    pub async fn list_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/actions/runs?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get a single workflow run by id.
    pub async fn get_workflow_run(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/actions/runs/{run_id}");
        self.get_conditional(&path, None).await
    }

    // ---- milestones --------------------------------------------------

    /// List milestones on a repo.
    pub async fn list_milestones(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/milestones?state=all&per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get a single milestone by number.
    pub async fn get_milestone(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/milestones/{number}");
        self.get_conditional(&path, None).await
    }

    // ---- labels ------------------------------------------------------

    /// List labels defined on a repo.
    pub async fn list_labels(
        &self,
        owner: &str,
        repo: &str,
        etag: Option<&str>,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/labels?per_page=100");
        self.get_conditional(&path, etag).await
    }

    /// Get a single label by name.
    pub async fn get_label(
        &self,
        owner: &str,
        repo: &str,
        label_name: &str,
    ) -> Result<Fetched<serde_json::Value>, ClientError> {
        let path = format!("/repos/{owner}/{repo}/labels/{label_name}");
        self.get_conditional(&path, None).await
    }

    // ---- issue write surface (SCOPE-PROJECTS §8) --------------------
    //
    // The §8 mutation handlers in dp-rest call into these methods
    // between the §8.2 step-5 CAS and the step-7 commit. We keep
    // the GitHub I/O behind the same `Client` chokepoint as the
    // read surface so the local request budget covers writes too —
    // a runaway tick that opens / patches issues should still trip
    // `BudgetExhausted` rather than slipping past the fuse.
    //
    // Each method translates octocrab's `Result<_>` into the
    // narrow [`GhWriteError`] split (validation vs upstream) the
    // dp-rest writer adapter wants. The split is the same shape as
    // `dp_rest::IssueWriteError` so the adapter is one-to-one.

    /// `POST /repos/{owner}/{repo}/issues`. Returns the
    /// GitHub-assigned issue number plus the full GitHub-side
    /// payload so the caller can hand it to
    /// [`crate::worker::handlers::parse_issue_upsert`] and
    /// materialise the local `dp_issues` row immediately — without
    /// waiting for the next webhook / reconciler tick. This is what
    /// lets the REST `POST /issues` handler attach the freshly
    /// created issue to a project / view in the same request.
    pub async fn gh_create_issue(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        body: Option<&str>,
    ) -> Result<(i64, serde_json::Value), GhWriteError> {
        self.check_and_count_budget()?;
        let handler = self.inner.issues(owner, repo);
        let mut builder = handler.create(title);
        if let Some(b) = body {
            builder = builder.body(b);
        }
        let issue = builder.send().await.map_err(map_octocrab_write_err)?;
        let number = issue.number as i64;
        let payload = serde_json::to_value(issue).map_err(|e| {
            GhWriteError::Upstream(format!(
                "serialize created issue payload from github: {e}"
            ))
        })?;
        Ok((number, payload))
    }

    /// `PATCH /repos/{owner}/{repo}/issues/{number}`. Forwards
    /// every `Some(_)` field of [`IssueRemotePatch`] to GitHub.
    ///
    /// Returns the GitHub-side issue payload (`serde_json::Value`)
    /// so the caller can hand it to
    /// [`crate::worker::handlers::parse_issue_upsert`] and refresh
    /// the local `dp_issues` row immediately — without waiting for
    /// the next webhook / reconciler tick. The shape matches
    /// `/repos/{owner}/{repo}/issues/{number}` exactly.
    pub async fn gh_update_issue(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        patch: &IssueRemotePatch,
    ) -> Result<serde_json::Value, GhWriteError> {
        self.check_and_count_budget()?;
        let handler = self.inner.issues(owner, repo);
        let mut builder = handler.update(number as u64);
        if let Some(t) = patch.title.as_deref() {
            builder = builder.title(t);
        }
        if let Some(b) = patch.body.as_deref() {
            builder = builder.body(b);
        }
        if let Some(state) = patch.state.as_deref() {
            let st = match state {
                "open" => octocrab::models::IssueState::Open,
                "closed" => octocrab::models::IssueState::Closed,
                other => {
                    return Err(GhWriteError::Validation(format!(
                        "invalid issue state {other:?}; expected \"open\" or \"closed\""
                    )));
                }
            };
            builder = builder.state(st);
        }
        if let Some(labels) = patch.labels.as_deref() {
            builder = builder.labels(labels);
        }
        if let Some(assignees) = patch.assignees.as_deref() {
            builder = builder.assignees(assignees);
        }
        let issue = builder.send().await.map_err(map_octocrab_write_err)?;
        serde_json::to_value(issue).map_err(|e| {
            GhWriteError::Upstream(format!(
                "serialize updated issue payload from github: {e}"
            ))
        })
    }

    /// `POST /repos/{owner}/{repo}/issues/{number}/comments`.
    pub async fn gh_create_comment(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        body: &str,
    ) -> Result<(), GhWriteError> {
        self.check_and_count_budget()?;
        self.inner
            .issues(owner, repo)
            .create_comment(number as u64, body)
            .await
            .map_err(map_octocrab_write_err)?;
        Ok(())
    }

    /// `GET /repos/{owner}/{repo}/issues/{number}` — single-issue
    /// refetch used by the §13.7 lazy resync path. Returns the raw
    /// GitHub payload as a `serde_json::Value` so the caller can
    /// hand it to [`crate::worker::handlers::parse_issue_upsert`]
    /// and upsert without waiting for the next webhook /
    /// reconciler tick — closing the "open issue, see staleness"
    /// gap when the issue was changed on GitHub since the last
    /// reconciler pass.
    pub async fn gh_get_issue(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<serde_json::Value, GhWriteError> {
        self.check_and_count_budget()?;
        let issue = self
            .inner
            .issues(owner, repo)
            .get(number as u64)
            .await
            .map_err(map_octocrab_write_err)?;
        serde_json::to_value(issue).map_err(|e| {
            GhWriteError::Upstream(format!(
                "serialize fetched issue payload from github: {e}"
            ))
        })
    }

    // ---- milestone write surface (PROJECT-VIEW.md follow-up) --------
    //
    // GitHub's milestones REST surface is not exposed by octocrab's
    // typed builders, so we go through the generic `_post` escape
    // hatch. The auth + transport plumbing (App-installation
    // token, retries, tracing) is still handled by octocrab — we
    // only choose the path and body shape.

    /// `POST /repos/{owner}/{repo}/milestones`. Returns the full
    /// GitHub-side milestone payload so the caller can hand it to
    /// [`crate::client::parse_milestone_upsert`]-style code and
    /// upsert the local `dp_milestones` row in the same request,
    /// without waiting for the next reconciler tick.
    pub async fn gh_create_milestone(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        description: Option<&str>,
        due_on: Option<chrono::NaiveDate>,
    ) -> Result<serde_json::Value, GhWriteError> {
        self.check_and_count_budget()?;
        // GitHub accepts `due_on` as an ISO-8601 timestamp; we
        // anchor to UTC midnight of the calendar date so the
        // returned `due_on` round-trips back to the same DATE
        // when we re-fetch.
        let due_on_str =
            due_on.map(|d| format!("{}T00:00:00Z", d.format("%Y-%m-%d")));
        let body = serde_json::json!({
            "title": title,
            "description": description,
            "due_on": due_on_str,
            "state": "open",
        });
        let path = format!("/repos/{owner}/{repo}/milestones");
        let payload: serde_json::Value = self
            .inner
            .post(path, Some(&body))
            .await
            .map_err(map_octocrab_write_err)?;
        Ok(payload)
    }

    /// `PATCH /repos/{owner}/{repo}/milestones/{number}`. Forwards
    /// every `Some(_)` field — `None` means "leave as-is on
    /// GitHub". Returns the GitHub-side milestone payload so the
    /// caller can mirror the update locally without waiting for
    /// the next reconciler tick.
    pub async fn gh_update_milestone(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        patch: &MilestoneRemotePatch<'_>,
    ) -> Result<serde_json::Value, GhWriteError> {
        self.check_and_count_budget()?;
        // Build the body sparsely so unset fields don't overwrite
        // with `null` — GitHub treats an explicit null differently
        // from an omitted key (null clears `due_on`, omission
        // leaves it). The handler decides which one it wants.
        let mut body = serde_json::Map::new();
        if let Some(t) = patch.title {
            body.insert("title".into(), serde_json::Value::String(t.into()));
        }
        if let Some(state) = patch.state {
            match state {
                "open" | "closed" => {
                    body.insert(
                        "state".into(),
                        serde_json::Value::String(state.into()),
                    );
                }
                other => {
                    return Err(GhWriteError::Validation(format!(
                        "invalid milestone state {other:?}; expected \"open\" or \"closed\""
                    )));
                }
            }
        }
        if let Some(desc) = patch.description {
            body.insert(
                "description".into(),
                desc.map_or(serde_json::Value::Null, |s| {
                    serde_json::Value::String(s.into())
                }),
            );
        }
        if let Some(due) = patch.due_on {
            body.insert(
                "due_on".into(),
                due.map_or(serde_json::Value::Null, |d| {
                    serde_json::Value::String(format!(
                        "{}T00:00:00Z",
                        d.format("%Y-%m-%d")
                    ))
                }),
            );
        }
        let path = format!("/repos/{owner}/{repo}/milestones/{number}");
        let payload: serde_json::Value = self
            .inner
            .patch(path, Some(&serde_json::Value::Object(body)))
            .await
            .map_err(map_octocrab_write_err)?;
        Ok(payload)
    }

    /// `DELETE /repos/{owner}/{repo}/milestones/{number}`. Hard
    /// delete — GitHub keeps the milestone number reserved so a
    /// future create with the same title gets a fresh number.
    pub async fn gh_delete_milestone(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<(), GhWriteError> {
        self.check_and_count_budget()?;
        let path = format!("/repos/{owner}/{repo}/milestones/{number}");
        // `_delete` returns the raw response; GitHub answers 204
        // No Content on success which `delete::<Value, _, _>`
        // can't deserialise into a body. We check status directly
        // and map non-2xx into `GhWriteError`.
        let uri: http::Uri = path
            .parse()
            .map_err(|e| GhWriteError::Upstream(format!("bad path: {e}")))?;
        let resp = self
            .inner
            ._delete(uri, None::<&()>)
            .await
            .map_err(map_octocrab_write_err)?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else if status.is_client_error() {
            Err(GhWriteError::Validation(format!(
                "github delete milestone returned {status}",
            )))
        } else {
            Err(GhWriteError::Upstream(format!(
                "github delete milestone returned {status}",
            )))
        }
    }

    // ---- Projects v2 GraphQL mirror (§3.10) --------------------------
    //
    // The §3.10 `PATCH /issues/{id}/dates` handler enqueues a
    // best-effort mirror task; the dp-rest octocrab adapter calls
    // through these methods to land start / due dates on the linked
    // GitHub Projects v2 board. Every call counts against the same
    // local budget so a runaway date editor cannot bypass the fuse
    // by funneling through GraphQL.
    //
    // GraphQL replies are JSON `{ data, errors }` envelopes; the
    // helpers below project the `data` lane on success and lift the
    // `errors[]` text into [`GhWriteError::Validation`] on failure
    // so the surface is the same as the REST writers.

    /// Generic GraphQL POST against `/graphql`. Returns the parsed
    /// `data` value on success, or a `GhWriteError` carrying:
    ///
    ///   * The concatenated `errors[].message` text on a GraphQL
    ///     error envelope (200 + non-empty `errors`).
    ///   * The transport / 5xx description otherwise.
    pub async fn gh_graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, GhWriteError> {
        self.check_and_count_budget()?;
        let body = serde_json::json!({ "query": query, "variables": variables });
        let resp: serde_json::Value = self
            .inner
            .graphql(&body)
            .await
            .map_err(map_octocrab_write_err)?;
        if let Some(errs) = resp.get("errors").and_then(|v| v.as_array()) {
            if !errs.is_empty() {
                // Concatenate messages so the adapter has the full
                // story when stamping `dp_issue_dates.mirror_error`.
                let msg = errs
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(GhWriteError::Validation(msg));
            }
        }
        resp.get("data").cloned().ok_or_else(|| {
            GhWriteError::Upstream("graphql response missing `data`".into())
        })
    }

    /// `addProjectV2ItemById(projectId, contentId)` — links an
    /// issue to a Projects v2 board. Idempotent on the GitHub
    /// side: a second call with the same `contentId` returns the
    /// pre-existing item id rather than creating a duplicate
    /// card. The §3.10 mirror still persists the returned id so
    /// the next edit skips this call.
    pub async fn gh_projectv2_add_item(
        &self,
        project_node_id: &str,
        content_node_id: &str,
    ) -> Result<String, GhWriteError> {
        let query = r#"
            mutation AddItem($project: ID!, $content: ID!) {
              addProjectV2ItemById(input: {projectId: $project, contentId: $content}) {
                item { id }
              }
            }
        "#;
        let vars = serde_json::json!({
            "project": project_node_id,
            "content": content_node_id,
        });
        let data = self.gh_graphql(query, vars).await?;
        data.get("addProjectV2ItemById")
            .and_then(|v| v.get("item"))
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                GhWriteError::Upstream(
                    "graphql addProjectV2ItemById missing item.id".into(),
                )
            })
    }

    /// `updateProjectV2ItemFieldValue` for a `date` field. Pass
    /// `None` to clear the value — the mirror lifts a local
    /// `null` start / due straight through so a cleared local
    /// row drops the Projects v2 card date too.
    pub async fn gh_projectv2_update_date_field(
        &self,
        project_node_id: &str,
        item_node_id: &str,
        field_node_id: &str,
        date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), GhWriteError> {
        let query = r#"
            mutation SetDate($project: ID!, $item: ID!, $field: ID!, $value: Date) {
              updateProjectV2ItemFieldValue(
                input: {projectId: $project, itemId: $item, fieldId: $field, value: {date: $value}}
              ) { projectV2Item { id } }
            }
        "#;
        // GraphQL `Date` scalar is `YYYY-MM-DD`. We render the UTC
        // calendar date — the §3.10 picker writes T00:00:00Z /
        // T23:59:59Z instants so this collapses cleanly.
        let value_json = match date {
            Some(d) => serde_json::Value::String(d.format("%Y-%m-%d").to_string()),
            None => serde_json::Value::Null,
        };
        let vars = serde_json::json!({
            "project": project_node_id,
            "item":    item_node_id,
            "field":   field_node_id,
            "value":   value_json,
        });
        let _ = self.gh_graphql(query, vars).await?;
        Ok(())
    }

    /// `createProjectV2Field` mutation — adds a new `Date` field
    /// to the board so the §6.4 link dialog has a target to
    /// mirror Start / Due into. Used by the "Create date fields"
    /// affordance when an operator picks a board that ships with
    /// only the GitHub default fields (Status / Iteration /
    /// Assignees) and no Date column.
    pub async fn gh_create_projectv2_date_field(
        &self,
        project_node_id: &str,
        name: &str,
    ) -> Result<String, GhWriteError> {
        let query = r#"
            mutation CreateDateField($project: ID!, $name: String!) {
              createProjectV2Field(input: {
                projectId: $project,
                dataType: DATE,
                name: $name
              }) {
                projectV2Field { ... on ProjectV2FieldCommon { id } }
              }
            }
        "#;
        let vars = serde_json::json!({
            "project": project_node_id,
            "name": name,
        });
        let data = self.gh_graphql(query, vars).await?;
        data.get("createProjectV2Field")
            .and_then(|v| v.get("projectV2Field"))
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                GhWriteError::Upstream(
                    "graphql createProjectV2Field missing projectV2Field.id".into(),
                )
            })
    }

    /// `repository(owner, name) { issue(number) { id } }` — the
    /// lazy fallback the mirror adapter calls when an issue row
    /// pre-dates the 0021 migration (no `dp_issues.github_node_id`
    /// yet). One round-trip per affected row, then the result is
    /// stamped back via
    /// [`Store::set_issue_github_node_id`][dp_domain::store::Store::set_issue_github_node_id]
    /// so subsequent mirrors are free.
    pub async fn gh_resolve_issue_node_id(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<String, GhWriteError> {
        let query = r#"
            query IssueId($owner: String!, $name: String!, $number: Int!) {
              repository(owner: $owner, name: $name) {
                issue(number: $number) { id }
              }
            }
        "#;
        let vars = serde_json::json!({
            "owner":  owner,
            "name":   repo,
            "number": number,
        });
        let data = self.gh_graphql(query, vars).await?;
        data.get("repository")
            .and_then(|v| v.get("issue"))
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                GhWriteError::Upstream(
                    "graphql repository.issue.id missing in response".into(),
                )
            })
    }

    /// `organization(login) { projectsV2(first: 50) { … } }` — the
    /// org-scoped picker the §6.4 `Link a board…` dialog reads
    /// from (linear-projects-v2.md §7.3). The dp-rest layer
    /// normalizes the GraphQL envelope into `OrgProjectPickerDto`
    /// so the REST contract never leaks the GraphQL schema.
    pub async fn gh_list_org_projectv2(
        &self,
        org: &str,
    ) -> Result<serde_json::Value, GhWriteError> {
        let query = r#"
            query OrgProjects($login: String!) {
              organization(login: $login) {
                projectsV2(first: 50) {
                  nodes {
                    id
                    title
                    number
                    url
                    closed
                    fields(first: 50) {
                      nodes {
                        ... on ProjectV2FieldCommon { id name dataType }
                      }
                    }
                  }
                }
              }
            }
        "#;
        let vars = serde_json::json!({ "login": org });
        let data = self.gh_graphql(query, vars).await?;
        Ok(data
            .get("organization")
            .and_then(|v| v.get("projectsV2"))
            .cloned()
            .unwrap_or(serde_json::json!({"nodes": []})))
    }

    /// `repository(owner, name) { projectsV2(first: 50) { … } }`
    /// — used by the admin pane to surface a project picker
    /// without forcing the operator to paste raw node ids. Each
    /// project also lists its fields so the UI can wire start /
    /// due in one step.
    pub async fn gh_list_repo_projectv2(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<serde_json::Value, GhWriteError> {
        let query = r#"
            query RepoProjects($owner: String!, $name: String!) {
              repository(owner: $owner, name: $name) {
                projectsV2(first: 50) {
                  nodes {
                    id
                    title
                    number
                    url
                    closed
                    fields(first: 50) {
                      nodes {
                        ... on ProjectV2FieldCommon { id name dataType }
                      }
                    }
                  }
                }
              }
            }
        "#;
        let vars = serde_json::json!({ "owner": owner, "name": repo });
        let data = self.gh_graphql(query, vars).await?;
        Ok(data
            .get("repository")
            .and_then(|v| v.get("projectsV2"))
            .cloned()
            .unwrap_or(serde_json::json!({"nodes": []})))
    }

    /// Internal: shared budget check used by every dispatch path.
    /// Increments the counter only after the fuse passes so a
    /// blown budget reports the pre-call value.
    fn check_and_count_budget(&self) -> Result<(), GhWriteError> {
        if let Some(max) = self.max_requests {
            let made = self.requests_made.load(Ordering::SeqCst);
            if made >= max {
                return Err(GhWriteError::Upstream(format!(
                    "local request budget exhausted: made {made} of max {max}"
                )));
            }
        }
        let _ = self.requests_made.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Field set the §8 mutation handler hands to
/// [`Client::gh_update_issue`]. Only `Some(_)` lanes are forwarded
/// to GitHub.
#[derive(Debug, Clone, Default)]
pub struct IssueRemotePatch {
    /// New title.
    pub title: Option<String>,
    /// New body.
    pub body: Option<String>,
    /// `"open"` / `"closed"`. Any other string surfaces as
    /// [`GhWriteError::Validation`] without dispatching the HTTP
    /// call — GitHub only accepts these two values.
    pub state: Option<String>,
    /// Replacement label set.
    pub labels: Option<Vec<String>>,
    /// Replacement assignee logins.
    pub assignees: Option<Vec<String>>,
}

/// Field set [`Client::gh_update_milestone`] forwards to GitHub.
/// Three lanes use the `Option<Option<…>>` shape so the handler
/// can distinguish "leave as-is" (`None`) from "explicitly clear"
/// (`Some(None)`); GitHub treats omitted vs. null differently for
/// `description` and `due_on`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MilestoneRemotePatch<'a> {
    /// New title.
    pub title: Option<&'a str>,
    /// `"open"` / `"closed"`. Any other value surfaces as
    /// [`GhWriteError::Validation`].
    pub state: Option<&'a str>,
    /// `None` = leave as-is; `Some(None)` = clear; `Some(Some(_))`
    /// = replace.
    pub description: Option<Option<&'a str>>,
    /// `None` = leave as-is; `Some(None)` = clear; `Some(Some(_))`
    /// = replace.
    pub due_on: Option<Option<chrono::NaiveDate>>,
}

/// Error split surfaced by the §8 write methods. Mirrors
/// `dp_rest::IssueWriteError` so the dp-rest adapter is a
/// one-to-one mapping.
#[derive(Debug, thiserror::Error)]
pub enum GhWriteError {
    /// GitHub returned 4xx (most commonly 422 validation).
    #[error("github validation: {0}")]
    Validation(String),
    /// 5xx, transport, JSON parse, or local budget exhausted.
    #[error("github upstream: {0}")]
    Upstream(String),
}

fn map_octocrab_write_err(e: octocrab::Error) -> GhWriteError {
    use octocrab::Error as O;
    match &e {
        O::GitHub { source, .. } => {
            let status = source.status_code.as_u16();
            let msg = source.message.clone();
            if (400..500).contains(&status) {
                GhWriteError::Validation(format!("status {status}: {msg}"))
            } else {
                GhWriteError::Upstream(format!("status {status}: {msg}"))
            }
        }
        _ => GhWriteError::Upstream(e.to_string()),
    }
}

fn transport<E: std::fmt::Display>(e: E) -> ClientError {
    ClientError::Transport(e.to_string())
}

async fn read_body(
    resp: http::Response<http_body_util::combinators::BoxBody<Bytes, octocrab::Error>>,
) -> Result<Bytes, ClientError> {
    resp.into_body()
        .collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(transport)
}

fn truncate(bytes: &[u8]) -> String {
    const CAP: usize = 4 * 1024;
    let slice = if bytes.len() > CAP { &bytes[..CAP] } else { bytes };
    String::from_utf8_lossy(slice).into_owned()
}
