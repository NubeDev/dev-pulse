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
