//! Per-resource-kind handlers that translate a queued
//! [`WebhookDelivery`] into idempotent store writes.
//!
//! Dispatch is keyed off the `X-GitHub-Event` value the receiver
//! captured into [`WebhookDelivery::event`]. Each handler:
//!
//! 1. Upserts the org + repo (or just the org, for org-scoped
//!    events) extracted from the payload — webhooks are the
//!    source-of-truth for "which entities exist", so the worker
//!    creates them on demand. Upserts are idempotent on
//!    `(github_id)` / `(org_id, github_id)` per TODO §0.3.
//! 2. Upserts every actor user found in the payload and produces
//!    a `(user_id, role)` list per SCOPE §6 + TODO §0.2.
//! 3. Calls [`Store::record_event`] keyed on `external_id` so
//!    redeliveries (or a reconciler pass that catches the same
//!    object) collapse into one row, then attaches the actors.
//!
//! ## Edge cases the unit tests pin
//!
//! * **Co-authored commits** — `Co-authored-by:` trailers in the
//!   commit message produce extra `CoAuthor`-role rows. The
//!   trailer parser lives in [`super::trailers`].
//! * **Squash merge** — the merged-PR event gets `Author` (the PR
//!   opener), `Committer` (often the same user), and `Merger`
//!   (the user who pressed the button) as three distinct rows.
//! * **Bot accounts** — surfaced as ordinary users; the report
//!   layer filters them by `login` suffix `[bot]`. We do not
//!   drop them at ingest because SCOPE §6 wants them "tracked
//!   separately", not lost.
//! * **Unattributed commits** — a commit with no `username` (the
//!   email did not resolve to a GitHub login at GitHub's side)
//!   produces an [`ActivityEvent`] with **no** `Author` actor
//!   row. The report layer surfaces these via the "unattributed"
//!   bucket; double-counting is avoided because there's no
//!   `user_id` to dedup against.
//!
//! Anything we can't parse is a [`HandlerError`]; the worker
//! captures the message into `webhook_inbox.error` and leaves
//! `processed_at` NULL so the row stays claimable on the next
//! drain — GitHub's at-least-once delivery is the safety net.

use chrono::{DateTime, Utc};
use dp_domain::{
    ActivityEvent, ActorRole, EventActor, EventKind, IssueState, IssueUpsert,
    IssueUpsertOutcome, Membership, MembershipRole, Org, Repo, Store, StoreError, Team, User,
    WebhookDelivery,
};
use serde_json::Value;
use uuid::Uuid;

use super::trailers::parse_coauthors;

/// Errors a handler can surface. Mapped 1:1 into
/// `webhook_inbox.error` text by the drain loop.
#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    /// Required JSON field missing or wrong type. The string is a
    /// JSON-path-ish locator (e.g. `repository.owner.id`).
    #[error("payload missing field: {0}")]
    MissingField(&'static str),
    /// The event was recognised but the action subkind is one we
    /// don't yet care about (e.g. `pull_request.action = "labeled"`).
    /// Worker treats this as success and marks processed.
    #[error("ignored action: {kind}/{action}")]
    Ignored {
        /// `X-GitHub-Event` value.
        kind: String,
        /// `payload.action` value.
        action: String,
    },
    /// Underlying store error.
    #[error("store: {0}")]
    Store(#[from] StoreError),
    /// Timestamp string failed to parse as RFC3339.
    #[error("bad timestamp at {0}")]
    BadTimestamp(&'static str),
}

impl HandlerError {
    /// `true` if the worker should mark the row processed (rather
    /// than failed) — for now only [`HandlerError::Ignored`].
    pub fn is_benign(&self) -> bool {
        matches!(self, HandlerError::Ignored { .. })
    }
}

/// What [`apply_delivery`] reports back. Lets the worker count
/// fan-out — one delivery can produce many actor rows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HandlerOutcome {
    /// Number of `record_event` rows the handler wrote.
    pub events: u32,
    /// Number of `event_actors` rows attached.
    pub actors: u32,
}

/// Dispatch one delivery to the correct per-event handler.
///
/// The string match is on the `X-GitHub-Event` value captured at
/// receive time, which is exactly what GitHub sends. Unknown
/// events are surfaced as [`HandlerError::Ignored`] so they get
/// marked processed (we already validated the HMAC; there's
/// nothing more to do).
pub async fn apply_delivery(
    store: &dyn Store,
    delivery: &WebhookDelivery,
) -> Result<HandlerOutcome, HandlerError> {
    let p = &delivery.payload;
    match delivery.event.as_str() {
        "pull_request" => handle_pull_request(store, p).await,
        "pull_request_review" => handle_pr_review(store, p).await,
        "pull_request_review_comment" => handle_pr_review_comment(store, p).await,
        "issues" => handle_issues(store, p).await,
        "issue_comment" => handle_issue_comment(store, p).await,
        "push" => handle_push(store, p).await,
        "workflow_run" => handle_workflow_run(store, p).await,
        "deployment" => handle_deployment(store, p).await,
        "release" => handle_release(store, p).await,
        "member" => handle_member(store, p).await,
        "membership" => handle_membership(store, p).await,
        "team" => handle_team(store, p).await,
        other => Err(HandlerError::Ignored {
            kind: other.to_string(),
            action: action_str(p).unwrap_or("").to_string(),
        }),
    }
}

// ---------- shared upsert helpers ----------------------------------

/// Upsert the `repository` block; returns `(org_id, repo_id)`.
async fn upsert_repo_from_payload(
    store: &dyn Store,
    p: &Value,
) -> Result<(Uuid, Uuid), HandlerError> {
    let repo_v = p
        .get("repository")
        .ok_or(HandlerError::MissingField("repository"))?;
    let owner_v = repo_v
        .get("owner")
        .ok_or(HandlerError::MissingField("repository.owner"))?;
    let org_id = upsert_org_from(store, owner_v).await?;
    let repo_gid = repo_v
        .get("id")
        .and_then(Value::as_i64)
        .ok_or(HandlerError::MissingField("repository.id"))?;
    let repo_name = repo_v
        .get("name")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("repository.name"))?
        .to_string();
    let repo = store
        .upsert_repo(&Repo {
            id: Uuid::new_v4(),
            org_id,
            github_id: repo_gid,
            name: repo_name,
        })
        .await?;
    Ok((org_id, repo.id))
}

async fn upsert_org_from(store: &dyn Store, owner_v: &Value) -> Result<Uuid, HandlerError> {
    let org_gid = owner_v
        .get("id")
        .and_then(Value::as_i64)
        .ok_or(HandlerError::MissingField("organization/owner.id"))?;
    let org_login = owner_v
        .get("login")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("organization/owner.login"))?
        .to_string();
    let org = store
        .upsert_org(&Org {
            id: Uuid::new_v4(),
            github_id: org_gid,
            login: org_login,
            name: owner_v
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .await?;
    Ok(org.id)
}

/// Upsert a user from a GitHub `user` object (`{id, login, ...}`).
/// Returns `None` if the input is missing the required fields —
/// this is the "unattributed" path for push commits whose author
/// email did not resolve to a GitHub login.
async fn upsert_user_obj(store: &dyn Store, v: &Value) -> Result<Option<Uuid>, HandlerError> {
    let Some(gid) = v.get("id").and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(login) = v.get("login").and_then(Value::as_str) else {
        return Ok(None);
    };
    let user = store
        .upsert_user(&User {
            id: Uuid::new_v4(),
            github_id: gid,
            login: login.to_string(),
            email: v
                .get("email")
                .and_then(Value::as_str)
                .map(str::to_string),
            name: v.get("name").and_then(Value::as_str).map(str::to_string),
            deleted_at: None,
        })
        .await?;
    Ok(Some(user.id))
}

/// Look up a GitHub user by login alone — used for push-commit
/// authors which carry `{name, email, username}` rather than a
/// full user object. Synthesises a `github_id` from a hash of the
/// login if we have to create the row from scratch; the next
/// reconcile pass will replace it with the real id.
///
/// Returns `None` when `username` is `None` or empty (the
/// unattributed bucket).
async fn upsert_user_by_login(
    store: &dyn Store,
    login: Option<&str>,
    name: Option<&str>,
    email: Option<&str>,
) -> Result<Option<Uuid>, HandlerError> {
    let Some(login) = login.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    // If a row already exists for this login (e.g. the reconciler
    // has minted the canonical real-github_id row), reuse it. Stops
    // the co-author / noreply-login path from spawning a duplicate
    // synthetic row keyed on a negative `github_id` for a login
    // that already has a positive-id row in `dp_users`.
    if let Some(existing) = store.find_user_by_login(login).await? {
        return Ok(Some(existing.id));
    }
    // No row yet — mint a synthetic with a negative `github_id` so
    // we never collide with a real GitHub id (which are positive).
    // The reconciler later overwrites the row via upsert on the
    // real `github_id`, at which point a real positive id replaces
    // this one.
    let synth_id = -(crc32(login.as_bytes()) as i64 + 1);
    let user = store
        .upsert_user(&User {
            id: Uuid::new_v4(),
            github_id: synth_id,
            login: login.to_string(),
            email: email.map(str::to_string),
            name: name.map(str::to_string),
            deleted_at: None,
        })
        .await?;
    Ok(Some(user.id))
}

fn crc32(bytes: &[u8]) -> u32 {
    // Tiny inline CRC-32 (IEEE) so we don't pull a crate in just
    // for the synthetic-id hash. Deterministic + collision-rate
    // is good enough for a placeholder keyspace.
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn action_str(p: &Value) -> Option<&str> {
    p.get("action").and_then(Value::as_str)
}

fn parse_ts(p: &Value, path: &'static str) -> Result<DateTime<Utc>, HandlerError> {
    let s = pointer_str(p, path).ok_or(HandlerError::MissingField(path))?;
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| HandlerError::BadTimestamp(path))
}

fn pointer_str<'a>(p: &'a Value, dotted: &str) -> Option<&'a str> {
    let mut cur = p;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    cur.as_str()
}

async fn record(
    store: &dyn Store,
    org_id: Uuid,
    repo_id: Uuid,
    kind: EventKind,
    ts: DateTime<Utc>,
    external_id: String,
    payload: Value,
    actors: Vec<(Uuid, ActorRole)>,
) -> Result<HandlerOutcome, HandlerError> {
    let event = store
        .record_event(&ActivityEvent {
            id: Uuid::new_v4(),
            org_id,
            repo_id,
            kind,
            ts,
            external_id,
            payload,
        })
        .await?;
    let rows: Vec<EventActor> = actors
        .into_iter()
        .map(|(user_id, role)| EventActor {
            event_id: event.id,
            user_id,
            role,
        })
        .collect();
    let n_actors = rows.len() as u32;
    if !rows.is_empty() {
        store.add_event_actors(&rows).await?;
    }
    Ok(HandlerOutcome {
        events: 1,
        actors: n_actors,
    })
}

// ---------- pull_request -------------------------------------------

async fn handle_pull_request(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let pr = p
        .get("pull_request")
        .ok_or(HandlerError::MissingField("pull_request"))?;

    // Three actions become activity events; everything else is
    // accepted-and-ignored (the worker still marks processed so
    // the row leaves the inbox).
    let (kind, ts) = match action {
        "opened" => (EventKind::PullRequestOpened, parse_ts(pr, "created_at")?),
        "closed" => {
            if pr.get("merged").and_then(Value::as_bool).unwrap_or(false) {
                (EventKind::PullRequestMerged, parse_ts(pr, "merged_at")?)
            } else {
                (EventKind::PullRequestClosed, parse_ts(pr, "closed_at")?)
            }
        }
        other => {
            return Err(HandlerError::Ignored {
                kind: "pull_request".into(),
                action: other.into(),
            });
        }
    };

    let external_id = pr
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("pull_request.node_id"))?
        .to_string();

    let mut actors: Vec<(Uuid, ActorRole)> = Vec::new();
    if let Some(uid) = upsert_user_obj(store, pr.get("user").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Author));
    }
    // Assignees (multi).
    if let Some(arr) = pr.get("assignees").and_then(Value::as_array) {
        for a in arr {
            if let Some(uid) = upsert_user_obj(store, a).await? {
                actors.push((uid, ActorRole::Assignee));
            }
        }
    }
    // Requested reviewers (multi).
    if let Some(arr) = pr.get("requested_reviewers").and_then(Value::as_array) {
        for a in arr {
            if let Some(uid) = upsert_user_obj(store, a).await? {
                actors.push((uid, ActorRole::Requester));
            }
        }
    }

    if matches!(kind, EventKind::PullRequestMerged) {
        // Squash-merge split: author + committer + merger can all
        // differ (SCOPE §6). Author already added above.
        if let Some(uid) = upsert_user_obj(store, pr.get("merged_by").unwrap_or(&Value::Null)).await? {
            actors.push((uid, ActorRole::Merger));
        }
        // `committer` is rarely surfaced on the PR webhook itself,
        // but when GitHub does include it (some squash flows) we
        // honour it. Falls through silently otherwise.
        if let Some(uid) = upsert_user_obj(store, pr.get("committer").unwrap_or(&Value::Null)).await? {
            actors.push((uid, ActorRole::Committer));
        }
    }
    if matches!(kind, EventKind::PullRequestClosed) {
        // Closer != merger here (this branch is the not-merged
        // path). `sender` is the user who triggered the close.
        if let Some(uid) = upsert_user_obj(store, p.get("sender").unwrap_or(&Value::Null)).await? {
            actors.push((uid, ActorRole::Closer));
        }
    }

    record(
        store,
        org_id,
        repo_id,
        kind,
        ts,
        external_id,
        pr.clone(),
        actors,
    )
    .await
}

// ---------- pull_request_review ------------------------------------

async fn handle_pr_review(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action != "submitted" {
        return Err(HandlerError::Ignored {
            kind: "pull_request_review".into(),
            action: action.into(),
        });
    }
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let review = p
        .get("review")
        .ok_or(HandlerError::MissingField("review"))?;
    let ts = parse_ts(review, "submitted_at")?;
    let external_id = review
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("review.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, review.get("user").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Reviewer));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::Review,
        ts,
        external_id,
        review.clone(),
        actors,
    )
    .await
}

// ---------- pull_request_review_comment ----------------------------

async fn handle_pr_review_comment(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action != "created" {
        return Err(HandlerError::Ignored {
            kind: "pull_request_review_comment".into(),
            action: action.into(),
        });
    }
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let comment = p
        .get("comment")
        .ok_or(HandlerError::MissingField("comment"))?;
    let ts = parse_ts(comment, "created_at")?;
    let external_id = comment
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("comment.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, comment.get("user").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Commenter));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::ReviewComment,
        ts,
        external_id,
        comment.clone(),
        actors,
    )
    .await
}

// ---------- issues -------------------------------------------------

/// Best-effort defensive secondary window for the `dp_issues`
/// upsert's §13.7 reconciler guard. The *primary* guard runs in
/// `crate::reconciler::guard::apply_or_defer_delivery` before the
/// handler dispatches; this constant is the fallback that protects
/// against a TOCTOU between guard check and store write (a second
/// optimistic write that landed in the µs between the two).
///
/// Threading the real `issues.pending_remote_timeout_secs` through
/// the handler call graph is a follow-up: today the value sits in
/// `dp-config` and is read by the guard call site in the drain
/// loop, not by the handlers. The defensive window stays tight (a
/// minute) so a stale flag from a crashed mutation does not block
/// real ingest indefinitely.
const HANDLER_PENDING_REMOTE_FALLBACK_SECS: i64 = 60;

/// Parse a GitHub `issue` object into the [`IssueUpsert`] the
/// store layer consumes. Pulled out of [`handle_issues`] so the
/// REST-side backfill in `dp-cli` can call the same parser — the
/// shape of a webhook `payload.issue` and the items in
/// `GET /repos/{owner}/{repo}/issues` is identical (this is the
/// shared "issue" object in GitHub's API surface).
///
/// `org_id` / `repo_id` are resolved by the caller (the webhook
/// handler uses `upsert_repo_from_payload`; the backfill resolves
/// them from the loop's repo cursor) so the parser stays pure
/// and store-free.
pub fn parse_issue_upsert(
    org_id: Uuid,
    repo_id: Uuid,
    issue: &Value,
) -> Result<IssueUpsert, HandlerError> {
    let github_id = issue
        .get("id")
        .and_then(Value::as_i64)
        .ok_or(HandlerError::MissingField("issue.id"))?;
    // GitHub payloads always carry `node_id` on issues, but some
    // older test fixtures may not — keep this lenient so a
    // fixture without it ingests cleanly. Production payloads land
    // the column on `dp_issues.github_node_id` so the §3.10
    // Projects v2 mirror has the `contentId` without a follow-up
    // GraphQL lookup.
    let github_node_id = issue
        .get("node_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let number = issue
        .get("number")
        .and_then(Value::as_i64)
        .ok_or(HandlerError::MissingField("issue.number"))?;
    let title = issue
        .get("title")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("issue.title"))?
        .to_string();
    let body = issue
        .get("body")
        .and_then(Value::as_str)
        .map(str::to_string);
    let state_text = issue
        .get("state")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("issue.state"))?;
    let state = IssueState::from_str(state_text)
        .ok_or(HandlerError::MissingField("issue.state"))?;
    let labels = issue
        .get("labels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.get("name").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let assignees = issue
        .get("assignees")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("login").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let milestone = issue
        .get("milestone")
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let author = issue
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let state_reason = issue
        .get("state_reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = parse_ts(issue, "created_at")?;
    let updated_at = parse_ts(issue, "updated_at")?;
    let closed_at = match issue.get("closed_at") {
        Some(Value::String(s)) => Some(
            DateTime::parse_from_rfc3339(s)
                .map_err(|_| HandlerError::MissingField("issue.closed_at"))?
                .with_timezone(&Utc),
        ),
        _ => None,
    };
    Ok(IssueUpsert {
        org_id,
        repo_id,
        github_id,
        github_node_id,
        number,
        title,
        body,
        state,
        labels,
        assignees,
        milestone,
        author,
        state_reason,
        created_at,
        updated_at,
        closed_at,
    })
}

async fn handle_issues(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let issue = p
        .get("issue")
        .ok_or(HandlerError::MissingField("issue"))?;
    // GitHub's `/repos/{owner}/{repo}/issues` REST endpoint returns
    // both issues and pull requests; webhooks share the same
    // distinction. Skip PR rows — we mirror only the issue spine.
    if issue.get("pull_request").is_some() {
        return Err(HandlerError::Ignored {
            kind: "issues".into(),
            action: format!("{action} (pull_request payload)"),
        });
    }
    // Mirror the issue row into `dp_issues` regardless of action.
    // Slice-2 read endpoints (`/issues`, `/me/queue`) and the §5.5
    // filter pills all read from `dp_issues`; the activity-event
    // table the previous version of this handler only wrote to is
    // *also* needed (for slice-3 throughput / lead-time reports)
    // but is no longer sufficient on its own.
    let upsert = parse_issue_upsert(org_id, repo_id, issue)?;
    let window = chrono::Duration::seconds(HANDLER_PENDING_REMOTE_FALLBACK_SECS);
    match store.upsert_issue_from_github(&upsert, window).await? {
        (_, IssueUpsertOutcome::Deferred) => {
            // Concurrent §8 optimistic write is in flight. The
            // primary guard's drain loop would have caught this
            // for webhook deliveries; for backfill / re-delivery
            // we tolerate it and let the next sweep retry.
            tracing::debug!(
                target: "dp_fetcher::handlers",
                repo_id = %repo_id,
                number = upsert.number,
                "issue upsert deferred by §13.7 pending_remote guard"
            );
        }
        (_, IssueUpsertOutcome::Inserted | IssueUpsertOutcome::Updated | IssueUpsertOutcome::Skipped) => {}
    }

    // After the mirror lands, the activity-event slice keeps its
    // existing locked vocabulary — only `opened` / `closed` map to
    // an `EventKind`. Other actions (edited / assigned / labeled
    // / …) mutate the row but never emit an activity event, which
    // matches the slice-1 contract.
    let (kind, ts) = match action {
        "opened" => (EventKind::IssueOpened, parse_ts(issue, "created_at")?),
        "closed" => (EventKind::IssueClosed, parse_ts(issue, "closed_at")?),
        other => {
            return Err(HandlerError::Ignored {
                kind: "issues".into(),
                action: other.into(),
            });
        }
    };
    let external_id = issue
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("issue.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, issue.get("user").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Author));
    }
    if let Some(arr) = issue.get("assignees").and_then(Value::as_array) {
        for a in arr {
            if let Some(uid) = upsert_user_obj(store, a).await? {
                actors.push((uid, ActorRole::Assignee));
            }
        }
    }
    if matches!(kind, EventKind::IssueClosed) {
        // GitHub puts the user who closed an issue on `sender`.
        if let Some(uid) = upsert_user_obj(store, p.get("sender").unwrap_or(&Value::Null)).await? {
            actors.push((uid, ActorRole::Closer));
        }
    }
    record(
        store,
        org_id,
        repo_id,
        kind,
        ts,
        external_id,
        issue.clone(),
        actors,
    )
    .await
}

// ---------- issue_comment ------------------------------------------

async fn handle_issue_comment(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action != "created" {
        return Err(HandlerError::Ignored {
            kind: "issue_comment".into(),
            action: action.into(),
        });
    }
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let comment = p
        .get("comment")
        .ok_or(HandlerError::MissingField("comment"))?;
    let ts = parse_ts(comment, "created_at")?;
    let external_id = comment
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("comment.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, comment.get("user").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Commenter));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::IssueComment,
        ts,
        external_id,
        comment.clone(),
        actors,
    )
    .await
}

// ---------- push ---------------------------------------------------

async fn handle_push(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let empty: Vec<Value> = Vec::new();
    let commits = p
        .get("commits")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    let mut total_events = 0u32;
    let mut total_actors = 0u32;
    for c in commits {
        let sha = c
            .get("id")
            .and_then(Value::as_str)
            .ok_or(HandlerError::MissingField("commits[].id"))?
            .to_string();
        let ts = parse_ts(c, "timestamp")?;
        let message = c
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");

        let mut actors: Vec<(Uuid, ActorRole)> = Vec::new();
        // Primary author. `username` is the GitHub login, when
        // resolvable. Missing → unattributed (no Author row).
        if let Some(author) = c.get("author") {
            let uid = upsert_user_by_login(
                store,
                author.get("username").and_then(Value::as_str),
                author.get("name").and_then(Value::as_str),
                author.get("email").and_then(Value::as_str),
            )
            .await?;
            if let Some(uid) = uid {
                actors.push((uid, ActorRole::Author));
            }
        }
        // Committer (squash-merge split; same user as author when
        // not a squash).
        if let Some(committer) = c.get("committer") {
            let uid = upsert_user_by_login(
                store,
                committer.get("username").and_then(Value::as_str),
                committer.get("name").and_then(Value::as_str),
                committer.get("email").and_then(Value::as_str),
            )
            .await?;
            if let Some(uid) = uid {
                actors.push((uid, ActorRole::Committer));
            }
        }
        // Co-authored-by: trailers in the commit message footer.
        for ca in parse_coauthors(message) {
            // Prefer the noreply-login convention (avoids creating
            // a synthetic user when GitHub already gave us the
            // login in the email). Otherwise fall back to a
            // deterministic synthetic id keyed off the email.
            let login_opt = ca.noreply_login();
            let uid = upsert_user_by_login(
                store,
                login_opt.or_else(|| {
                    // No login at all → synthetic keyed off email
                    // so re-runs collapse. We pass the email itself
                    // as the "login" for the synthetic row.
                    Some(ca.email.as_str())
                }),
                Some(ca.name.as_str()),
                Some(ca.email.as_str()),
            )
            .await?;
            if let Some(uid) = uid {
                actors.push((uid, ActorRole::CoAuthor));
            }
        }

        let outcome = record(
            store,
            org_id,
            repo_id,
            EventKind::Commit,
            ts,
            sha,
            c.clone(),
            actors,
        )
        .await?;
        total_events += outcome.events;
        total_actors += outcome.actors;
    }
    Ok(HandlerOutcome {
        events: total_events,
        actors: total_actors,
    })
}

// ---------- workflow_run -------------------------------------------

async fn handle_workflow_run(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action != "completed" {
        return Err(HandlerError::Ignored {
            kind: "workflow_run".into(),
            action: action.into(),
        });
    }
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let run = p
        .get("workflow_run")
        .ok_or(HandlerError::MissingField("workflow_run"))?;
    let ts = parse_ts(run, "updated_at")?;
    let external_id = run
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("workflow_run.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    // `triggering_actor` is the human (or bot) that started the
    // run. `actor` exists too — surfaced here as Author for
    // attribution; the report layer can split if it cares.
    let actor_obj = run
        .get("triggering_actor")
        .or_else(|| run.get("actor"))
        .unwrap_or(&Value::Null);
    if let Some(uid) = upsert_user_obj(store, actor_obj).await? {
        actors.push((uid, ActorRole::Author));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::WorkflowRun,
        ts,
        external_id,
        run.clone(),
        actors,
    )
    .await
}

// ---------- deployment ---------------------------------------------

async fn handle_deployment(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let dep = p
        .get("deployment")
        .ok_or(HandlerError::MissingField("deployment"))?;
    let ts = parse_ts(dep, "created_at")?;
    let external_id = dep
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("deployment.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, dep.get("creator").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Author));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::Deployment,
        ts,
        external_id,
        dep.clone(),
        actors,
    )
    .await
}

// ---------- release ------------------------------------------------

async fn handle_release(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action != "published" {
        return Err(HandlerError::Ignored {
            kind: "release".into(),
            action: action.into(),
        });
    }
    let (org_id, repo_id) = upsert_repo_from_payload(store, p).await?;
    let rel = p
        .get("release")
        .ok_or(HandlerError::MissingField("release"))?;
    let ts = parse_ts(rel, "published_at")?;
    let external_id = rel
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("release.node_id"))?
        .to_string();
    let mut actors = Vec::new();
    if let Some(uid) = upsert_user_obj(store, rel.get("author").unwrap_or(&Value::Null)).await? {
        actors.push((uid, ActorRole::Author));
    }
    record(
        store,
        org_id,
        repo_id,
        EventKind::Release,
        ts,
        external_id,
        rel.clone(),
        actors,
    )
    .await
}

// ---------- member / membership / team -----------------------------
//
// These three update the user/org/team graph but do not surface as
// `activity_events` rows — there's no useful "report metric"
// keyed off them, only drift detection (TODO §0.1 reconciler).

async fn handle_member(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    // The `member` event fires for repo-level collaborator changes
    // (add/remove/edit). We surface it as a membership row against
    // the parent org so the user graph stays consistent; the
    // reconciler later refines role.
    let owner_v = p
        .get("repository")
        .and_then(|r| r.get("owner"))
        .or_else(|| p.get("organization"))
        .ok_or(HandlerError::MissingField("repository.owner/organization"))?;
    let org_id = upsert_org_from(store, owner_v).await?;
    let member = p
        .get("member")
        .ok_or(HandlerError::MissingField("member"))?;
    let user_id = upsert_user_obj(store, member)
        .await?
        .ok_or(HandlerError::MissingField("member.id"))?;
    if action == "removed" {
        // Soft-signal: we keep the membership row so historical
        // attribution still resolves, and rely on the reconciler
        // to materialise an `expired_at` column when we add one.
        return Ok(HandlerOutcome::default());
    }
    store
        .upsert_membership(&Membership {
            user_id,
            org_id,
            role: MembershipRole::Member,
            home_org: None,
            joined_at: Utc::now(),
        })
        .await?;
    Ok(HandlerOutcome::default())
}

async fn handle_membership(
    store: &dyn Store,
    p: &Value,
) -> Result<HandlerOutcome, HandlerError> {
    // `membership` fires when someone is added to / removed from a
    // *team*. We treat it the same as `member` for the org-level
    // membership table: the user belongs to the team's org.
    let org_v = p
        .get("organization")
        .ok_or(HandlerError::MissingField("organization"))?;
    let org_id = upsert_org_from(store, org_v).await?;
    let member = p
        .get("member")
        .ok_or(HandlerError::MissingField("member"))?;
    let user_id = upsert_user_obj(store, member)
        .await?
        .ok_or(HandlerError::MissingField("member.id"))?;
    let action = action_str(p).ok_or(HandlerError::MissingField("action"))?;
    if action == "removed" {
        return Ok(HandlerOutcome::default());
    }
    store
        .upsert_membership(&Membership {
            user_id,
            org_id,
            role: MembershipRole::Member,
            home_org: None,
            joined_at: Utc::now(),
        })
        .await?;
    Ok(HandlerOutcome::default())
}

async fn handle_team(store: &dyn Store, p: &Value) -> Result<HandlerOutcome, HandlerError> {
    let org_v = p
        .get("organization")
        .ok_or(HandlerError::MissingField("organization"))?;
    let org_id = upsert_org_from(store, org_v).await?;
    let team = p
        .get("team")
        .ok_or(HandlerError::MissingField("team"))?;
    let gid = team
        .get("id")
        .and_then(Value::as_i64)
        .ok_or(HandlerError::MissingField("team.id"))?;
    let slug = team
        .get("slug")
        .and_then(Value::as_str)
        .ok_or(HandlerError::MissingField("team.slug"))?
        .to_string();
    let name = team
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    store
        .upsert_team(&Team {
            id: Uuid::new_v4(),
            org_id,
            github_id: gid,
            slug,
            name,
        })
        .await?;
    Ok(HandlerOutcome::default())
}

#[cfg(test)]
mod tests {
    //! Fixture-driven tests for each handler. We trim GitHub's real
    //! payload shapes down to the fields the handler actually
    //! reads — the goal is "what does the worker do when GitHub
    //! says X", not "do we ignore the 200 fields we don't care
    //! about" (which the JSON parser does for free).
    use super::*;
    use crate::worker::test_store::FakeStore;
    use chrono::TimeZone;
    use serde_json::json;
    use std::sync::Arc;

    fn delivery(event: &str, payload: Value) -> WebhookDelivery {
        WebhookDelivery {
            id: Uuid::new_v4(),
            delivery_id: format!("d-{}", Uuid::new_v4()),
            event: event.into(),
            payload,
            received_at: Utc::now(),
            processed_at: None,
            error: None,
        }
    }

    fn repo_block() -> Value {
        json!({
            "id": 1001,
            "name": "dev-pulse",
            "owner": { "id": 42, "login": "nube-io" }
        })
    }

    #[tokio::test]
    async fn pull_request_opened_records_author_assignees_reviewers() {
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request",
            json!({
                "action": "opened",
                "repository": repo_block(),
                "pull_request": {
                    "node_id": "PR_kw1",
                    "created_at": "2024-01-02T03:04:05Z",
                    "user":       { "id": 7, "login": "alice" },
                    "assignees":  [ { "id": 8, "login": "bob" } ],
                    "requested_reviewers": [
                        { "id": 9,  "login": "carol" },
                        { "id": 10, "login": "dave"  }
                    ]
                }
            }),
        );
        let out = apply_delivery(s.as_ref(), &d).await.unwrap();
        assert_eq!(out.events, 1);
        assert_eq!(out.actors, 4); // author + 1 assignee + 2 requesters

        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::PullRequestOpened);
        assert_eq!(ev.external_id, "PR_kw1");
        assert_eq!(ev.ts, Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap());

        let actors = s.actors_for(ev.id);
        assert!(actors.iter().any(|(login, r)| login == "alice" && *r == ActorRole::Author));
        assert!(actors.iter().any(|(login, r)| login == "bob"   && *r == ActorRole::Assignee));
        assert!(actors.iter().any(|(login, r)| login == "carol" && *r == ActorRole::Requester));
        assert!(actors.iter().any(|(login, r)| login == "dave"  && *r == ActorRole::Requester));
    }

    #[tokio::test]
    async fn pull_request_merged_squash_records_author_merger_committer() {
        // Squash-merge: PR opened by alice, merged by bob,
        // committer recorded as carol (the rare case where GitHub
        // surfaces it on the PR webhook).
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request",
            json!({
                "action": "closed",
                "repository": repo_block(),
                "sender": { "id": 8, "login": "bob" },
                "pull_request": {
                    "node_id":   "PR_squash",
                    "merged":    true,
                    "merged_at": "2024-02-02T02:02:02Z",
                    "closed_at": "2024-02-02T02:02:02Z",
                    "created_at": "2024-02-01T01:01:01Z",
                    "user":      { "id": 7,  "login": "alice" },
                    "merged_by": { "id": 8,  "login": "bob"   },
                    "committer": { "id": 11, "login": "carol" }
                }
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::PullRequestMerged);
        let roles = s.roles_for_login(ev.id, "alice");
        assert!(roles.contains(&ActorRole::Author));
        let roles = s.roles_for_login(ev.id, "bob");
        assert!(roles.contains(&ActorRole::Merger));
        let roles = s.roles_for_login(ev.id, "carol");
        assert!(roles.contains(&ActorRole::Committer));
    }

    #[tokio::test]
    async fn pull_request_closed_without_merge_records_closer() {
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request",
            json!({
                "action": "closed",
                "repository": repo_block(),
                "sender": { "id": 99, "login": "closer-user" },
                "pull_request": {
                    "node_id":   "PR_x",
                    "merged":    false,
                    "closed_at": "2024-03-03T03:03:03Z",
                    "created_at": "2024-03-01T00:00:00Z",
                    "user":      { "id": 1, "login": "alice" }
                }
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::PullRequestClosed);
        assert!(s.roles_for_login(ev.id, "closer-user").contains(&ActorRole::Closer));
    }

    #[tokio::test]
    async fn pr_review_submitted_records_reviewer() {
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request_review",
            json!({
                "action": "submitted",
                "repository": repo_block(),
                "review": {
                    "node_id":     "RV_1",
                    "submitted_at":"2024-01-01T00:00:00Z",
                    "user":        { "id": 12, "login": "reviewer-1" },
                    "state":       "approved"
                }
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::Review);
        assert!(s.roles_for_login(ev.id, "reviewer-1").contains(&ActorRole::Reviewer));
    }

    #[tokio::test]
    async fn pr_review_comment_created_records_commenter() {
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "pull_request_review_comment",
            json!({
                "action": "created",
                "repository": repo_block(),
                "comment": {
                    "node_id":    "RVC_1",
                    "created_at": "2024-01-01T00:00:00Z",
                    "user":       { "id": 14, "login": "commenter" }
                }
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::ReviewComment);
        assert!(s.roles_for_login(ev.id, "commenter").contains(&ActorRole::Commenter));
    }

    #[tokio::test]
    async fn issue_opened_then_closed_emits_two_events_with_closer() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "issues",
                json!({
                    "action": "opened",
                    "repository": repo_block(),
                    "issue": {
                        "id":         9001,
                        "number":     1,
                        "node_id":    "I_1",
                        "title":      "first",
                        "state":      "open",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-01T00:00:00Z",
                        "user":       { "id": 1, "login": "alice" },
                        "assignees":  [ { "id": 2, "login": "bob" } ]
                    }
                }),
            ),
        )
        .await
        .unwrap();
        apply_delivery(
            s.as_ref(),
            &delivery(
                "issues",
                json!({
                    "action": "closed",
                    "repository": repo_block(),
                    "sender": { "id": 3, "login": "carol" },
                    "issue": {
                        "id":         9001,
                        "number":     1,
                        "node_id":    "I_1",
                        "title":      "first",
                        "state":      "closed",
                        "created_at": "2024-01-01T00:00:00Z",
                        "updated_at": "2024-01-02T00:00:00Z",
                        "closed_at":  "2024-01-02T00:00:00Z",
                        "user":       { "id": 1, "login": "alice" }
                    }
                }),
            ),
        )
        .await
        .unwrap();
        // The opened+closed events share an external_id but
        // different `kind`, so the unique-on-`(kind, external_id)`
        // constraint keeps them as two rows.
        assert_eq!(s.events_count(), 2);
        let closed = s
            .find_event_by_kind(EventKind::IssueClosed)
            .expect("closed event");
        assert!(s.roles_for_login(closed.id, "carol").contains(&ActorRole::Closer));
    }

    #[test]
    fn parse_issue_upsert_extracts_full_payload() {
        // Trimmed-down `/repos/{owner}/{repo}/issues` row — same
        // shape as the webhook `payload.issue`. We assert every
        // field the store impl persists so a future refactor of
        // the parser can't silently drop a column.
        let org_id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();
        let body = json!({
            "id": 4242,
            "number": 17,
            "title": "fix the thing",
            "body": "details",
            "state": "closed",
            "state_reason": "completed",
            "labels": [
                { "name": "bug" },
                { "name": "p1" }
            ],
            "assignees": [
                { "login": "alice" },
                { "login": "bob" }
            ],
            "milestone": { "title": "v0.2" },
            "user": { "login": "reporter" },
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-03T12:00:00Z",
            "closed_at":  "2024-01-03T12:00:00Z"
        });
        let u = parse_issue_upsert(org_id, repo_id, &body).unwrap();
        assert_eq!(u.org_id, org_id);
        assert_eq!(u.repo_id, repo_id);
        assert_eq!(u.github_id, 4242);
        assert_eq!(u.number, 17);
        assert_eq!(u.title, "fix the thing");
        assert_eq!(u.body.as_deref(), Some("details"));
        assert!(matches!(u.state, IssueState::Closed));
        assert_eq!(u.state_reason.as_deref(), Some("completed"));
        assert_eq!(u.labels, vec!["bug".to_string(), "p1".to_string()]);
        assert_eq!(u.assignees, vec!["alice".to_string(), "bob".to_string()]);
        assert_eq!(u.milestone.as_deref(), Some("v0.2"));
        assert_eq!(u.author.as_deref(), Some("reporter"));
        assert_eq!(
            u.created_at,
            chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            u.updated_at,
            chrono::Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap()
        );
        assert_eq!(
            u.closed_at.unwrap(),
            chrono::Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap()
        );
    }

    #[test]
    fn parse_issue_upsert_tolerates_open_issue_without_closed_at() {
        // Open issues come back with `closed_at: null`. Optional
        // fields (`body`, `milestone`, `state_reason`) are absent
        // entirely — the parser must accept both `null` and
        // missing-key for them.
        let u = parse_issue_upsert(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &json!({
                "id": 1,
                "number": 1,
                "title": "open one",
                "state": "open",
                "user": { "login": "alice" },
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-01-01T00:00:00Z",
                "closed_at":  null
            }),
        )
        .unwrap();
        assert!(matches!(u.state, IssueState::Open));
        assert!(u.body.is_none());
        assert!(u.milestone.is_none());
        assert!(u.state_reason.is_none());
        assert!(u.closed_at.is_none());
        assert!(u.labels.is_empty());
        assert!(u.assignees.is_empty());
    }

    #[test]
    fn parse_issue_upsert_rejects_unknown_state() {
        let err = parse_issue_upsert(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &json!({
                "id": 1, "number": 1, "title": "x",
                "state": "draft",
                "user": { "login": "a" },
                "created_at": "2024-01-01T00:00:00Z",
                "updated_at": "2024-01-01T00:00:00Z"
            }),
        )
        .unwrap_err();
        assert!(matches!(err, HandlerError::MissingField("issue.state")));
    }

    #[tokio::test]
    async fn issue_comment_records_commenter() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "issue_comment",
                json!({
                    "action": "created",
                    "repository": repo_block(),
                    "comment": {
                        "node_id":   "IC_1",
                        "created_at":"2024-01-01T00:00:00Z",
                        "user":      { "id": 1, "login": "alice" }
                    }
                }),
            ),
        )
        .await
        .unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::IssueComment);
        assert!(s.roles_for_login(ev.id, "alice").contains(&ActorRole::Commenter));
    }

    #[tokio::test]
    async fn push_with_coauthor_trailer_adds_extra_actor() {
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "push",
            json!({
                "repository": repo_block(),
                "commits": [
                    {
                        "id": "deadbeef00",
                        "timestamp": "2024-04-04T04:04:04Z",
                        "message": "feat: thing\n\nCo-authored-by: Octocat <12345+octocat@users.noreply.github.com>\n",
                        "author":    { "name": "Alice", "email": "alice@example.com", "username": "alice" },
                        "committer": { "name": "Alice", "email": "alice@example.com", "username": "alice" }
                    }
                ]
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::Commit);
        // alice gets both Author and Committer; octocat gets CoAuthor.
        let alice = s.roles_for_login(ev.id, "alice");
        assert!(alice.contains(&ActorRole::Author));
        assert!(alice.contains(&ActorRole::Committer));
        assert!(s
            .roles_for_login(ev.id, "octocat")
            .contains(&ActorRole::CoAuthor));
    }

    #[tokio::test]
    async fn push_with_unresolvable_author_lands_unattributed() {
        // `username` is missing — GitHub couldn't resolve the
        // email to a user. SCOPE §6 says "bucketed as unattributed"
        // — we record the event with **no** Author actor row.
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "push",
            json!({
                "repository": repo_block(),
                "commits": [
                    {
                        "id": "cafef00d00",
                        "timestamp": "2024-04-04T04:04:04Z",
                        "message": "wip",
                        "author":    { "name": "Anon", "email": "anon@example.com" },
                        "committer": { "name": "Anon", "email": "anon@example.com" }
                    }
                ]
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::Commit);
        let actors = s.actors_for(ev.id);
        assert!(actors.is_empty(), "unattributed: {actors:?}");
    }

    #[tokio::test]
    async fn push_with_bot_author_is_still_recorded() {
        // Bot accounts (`dependabot[bot]`) are first-class actor
        // rows — they're "tracked separately" per SCOPE §6, which
        // is a report-layer filter not an ingest-layer drop.
        let s = Arc::new(FakeStore::new());
        let d = delivery(
            "push",
            json!({
                "repository": repo_block(),
                "commits": [
                    {
                        "id": "b07b07b0",
                        "timestamp": "2024-04-04T04:04:04Z",
                        "message": "chore: bump deps",
                        "author":    { "name": "dependabot[bot]", "email": "bot@noreply", "username": "dependabot[bot]" },
                        "committer": { "name": "dependabot[bot]", "email": "bot@noreply", "username": "dependabot[bot]" }
                    }
                ]
            }),
        );
        apply_delivery(s.as_ref(), &d).await.unwrap();
        let ev = s.only_event();
        let roles = s.roles_for_login(ev.id, "dependabot[bot]");
        assert!(roles.contains(&ActorRole::Author));
        assert!(roles.contains(&ActorRole::Committer));
    }

    #[tokio::test]
    async fn workflow_run_completed_records_triggering_actor() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "workflow_run",
                json!({
                    "action": "completed",
                    "repository": repo_block(),
                    "workflow_run": {
                        "node_id":    "WR_1",
                        "updated_at": "2024-05-05T05:05:05Z",
                        "triggering_actor": { "id": 7, "login": "alice" }
                    }
                }),
            ),
        )
        .await
        .unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::WorkflowRun);
        assert!(s.roles_for_login(ev.id, "alice").contains(&ActorRole::Author));
    }

    #[tokio::test]
    async fn deployment_records_creator() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "deployment",
                json!({
                    "repository": repo_block(),
                    "deployment": {
                        "node_id":    "DEP_1",
                        "created_at": "2024-06-06T06:06:06Z",
                        "creator":    { "id": 7, "login": "alice" }
                    }
                }),
            ),
        )
        .await
        .unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::Deployment);
        assert!(s.roles_for_login(ev.id, "alice").contains(&ActorRole::Author));
    }

    #[tokio::test]
    async fn release_published_records_author() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "release",
                json!({
                    "action": "published",
                    "repository": repo_block(),
                    "release": {
                        "node_id":      "REL_1",
                        "published_at": "2024-07-07T07:07:07Z",
                        "author":       { "id": 7, "login": "alice" }
                    }
                }),
            ),
        )
        .await
        .unwrap();
        let ev = s.only_event();
        assert_eq!(ev.kind, EventKind::Release);
        assert!(s.roles_for_login(ev.id, "alice").contains(&ActorRole::Author));
    }

    #[tokio::test]
    async fn member_event_upserts_membership_no_activity_event() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "member",
                json!({
                    "action": "added",
                    "repository": repo_block(),
                    "member": { "id": 22, "login": "newbie" }
                }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(s.events_count(), 0);
        assert!(s.memberships_for_login("newbie").len() == 1);
    }

    #[tokio::test]
    async fn team_event_upserts_team() {
        let s = Arc::new(FakeStore::new());
        apply_delivery(
            s.as_ref(),
            &delivery(
                "team",
                json!({
                    "action": "created",
                    "organization": { "id": 42, "login": "nube-io" },
                    "team": { "id": 555, "slug": "backend", "name": "Backend" }
                }),
            ),
        )
        .await
        .unwrap();
        let teams = s.teams();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].slug, "backend");
    }

    #[tokio::test]
    async fn unknown_event_is_benign_ignored() {
        let s = Arc::new(FakeStore::new());
        let err = apply_delivery(
            s.as_ref(),
            &delivery("ping", json!({ "zen": "Practicality beats purity." })),
        )
        .await
        .unwrap_err();
        assert!(err.is_benign(), "{err:?}");
    }

    #[tokio::test]
    async fn redelivered_pr_is_idempotent_on_external_id() {
        // GitHub redelivered the same PR-opened event. The
        // worker's apply path must collapse to one event row +
        // one set of actor rows (the composite PK on
        // event_actors handles the actor side).
        let s = Arc::new(FakeStore::new());
        let payload = json!({
            "action": "opened",
            "repository": repo_block(),
            "pull_request": {
                "node_id":    "PR_dup",
                "created_at": "2024-08-08T08:08:08Z",
                "user":       { "id": 7, "login": "alice" }
            }
        });
        apply_delivery(s.as_ref(), &delivery("pull_request", payload.clone()))
            .await
            .unwrap();
        apply_delivery(s.as_ref(), &delivery("pull_request", payload))
            .await
            .unwrap();
        assert_eq!(s.events_count(), 1);
        let ev = s.only_event();
        // Author appears exactly once despite two deliveries.
        let n_author = s
            .actors_for(ev.id)
            .iter()
            .filter(|(login, r)| login == "alice" && *r == ActorRole::Author)
            .count();
        assert_eq!(n_author, 1);
    }
}
