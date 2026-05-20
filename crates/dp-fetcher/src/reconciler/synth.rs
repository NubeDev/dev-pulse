//! GitHub-list-response → webhook-shaped payload synthesis.
//!
//! The reconciler's "zero code duplication" promise (Stage 8) is
//! that it does **not** re-implement the upsert logic the webhook
//! worker already owns. Instead it takes the items GitHub returned
//! from the list endpoints (`/pulls`, `/issues`, `/commits`) and
//! shapes each one into the JSON the corresponding webhook event
//! would have carried, then hands the synthesised
//! [`WebhookDelivery`] to [`crate::worker::apply_delivery`].
//!
//! That keeps the multi-actor / co-author / bot / unattributed
//! handling — every edge case SCOPE §6 calls out — living in
//! exactly one place. The reconciler becomes a routing exercise:
//! "fetch list, synthesise, dispatch, advance cursor."
//!
//! ## Lossy by design
//!
//! Some webhook fields are simply not in the list-endpoint
//! response (e.g. `pull_request.merged_by` is not on `/pulls`; it
//! lives on the detail endpoint). When that happens we omit the
//! field and the handler treats it as "actor missing" — the worst
//! case is one missing actor row, which the next webhook delivery
//! or a detail-endpoint follow-up can backfill. This is acceptable
//! per SCOPE §11.4 trust: a reconciler's job is to *not lose
//! events*, not to surface every actor on the first pass.

use chrono::{DateTime, Utc};
use dp_domain::WebhookDelivery;
use serde_json::{json, Value};
use uuid::Uuid;

use super::targets::RepoTarget;

/// Shape a `{owner, repo}` block matching what the handler's
/// `upsert_repo_from_payload` expects.
fn repository_block(t: &RepoTarget) -> Value {
    json!({
        "id":   t.repo_github_id,
        "name": t.repo_name,
        "owner": {
            "id":    t.org_github_id,
            "login": t.owner_login,
        }
    })
}

/// Wrap a payload into a [`WebhookDelivery`] of the given event
/// type. `delivery_id` is synthesised with a `recon:` prefix so
/// the worker's idempotency log (and operators tailing tracing)
/// can spot reconciler-sourced events.
fn make_delivery(event: &str, payload: Value) -> WebhookDelivery {
    WebhookDelivery {
        id: Uuid::new_v4(),
        delivery_id: format!("recon:{}", Uuid::new_v4()),
        event: event.into(),
        payload,
        received_at: Utc::now(),
        processed_at: None,
        error: None,
    }
}

/// Synthesise zero, one, or two `pull_request` webhook deliveries
/// for a single PR returned by `GET /repos/{o}/{r}/pulls`.
///
/// - state=open → one `opened` delivery.
/// - state=closed, merged_at set → one `closed` delivery (merged=true).
/// - state=closed, no merged_at  → one `closed` delivery (merged=false).
///
/// The webhook stream would have sent an `opened` even for a PR
/// that later closed; we don't replay that because the handler's
/// `record_event` is idempotent on `(kind, external_id)` and the
/// "missed an `opened`" gap is already covered by the next reconciler
/// pass before the PR closes.
pub fn pulls_response_to_deliveries(t: &RepoTarget, pulls: &[Value]) -> Vec<WebhookDelivery> {
    let mut out = Vec::with_capacity(pulls.len());
    for pr in pulls {
        let state = pr.get("state").and_then(Value::as_str).unwrap_or("");
        let merged_at = pr.get("merged_at").and_then(Value::as_str);
        let merged = merged_at.is_some();

        // Mirror handler's required PR fields. Pass through what
        // the list endpoint gave us; missing optional fields stay
        // missing (the handler tolerates that).
        let mut pr_obj = pr.clone();
        if let Value::Object(map) = &mut pr_obj {
            if !map.contains_key("merged") {
                map.insert("merged".into(), Value::Bool(merged));
            }
        }

        let action = if state == "open" { "opened" } else { "closed" };

        out.push(make_delivery(
            "pull_request",
            json!({
                "action":       action,
                "repository":   repository_block(t),
                "pull_request": pr_obj,
                // `sender` is what the handler reads to attribute
                // the Closer role on a not-merged close. The list
                // endpoint doesn't include sender — handler reads
                // `upsert_user_obj(None)` → None → no Closer row,
                // which is acceptable per the "lossy by design"
                // contract above.
            }),
        ));
    }
    out
}

/// Synthesise one `issues` webhook delivery per non-PR issue
/// returned by `GET /repos/{o}/{r}/issues?since=`.
///
/// GitHub's issues list endpoint returns PRs too (every PR is also
/// an issue). We filter on `pull_request` being absent because the
/// PR list endpoint covers that resource; double-feeding the
/// handler is wasted work and would create a `Review` / `Author`
/// duplication on the report side.
pub fn issues_response_to_deliveries(t: &RepoTarget, issues: &[Value]) -> Vec<WebhookDelivery> {
    let mut out = Vec::with_capacity(issues.len());
    for issue in issues {
        if issue.get("pull_request").is_some() {
            continue;
        }
        let state = issue.get("state").and_then(Value::as_str).unwrap_or("");
        let action = if state == "open" { "opened" } else { "closed" };
        out.push(make_delivery(
            "issues",
            json!({
                "action":     action,
                "repository": repository_block(t),
                "issue":      issue,
            }),
        ));
    }
    out
}

/// Synthesise one `push` webhook delivery whose `commits` array is
/// every commit returned by `GET /repos/{o}/{r}/commits?since=`,
/// reshaped to match the push event format the handler reads.
///
/// The commits list endpoint returns
/// `{sha, commit: {author, committer, message}, author, committer}`
/// where the outer `author` / `committer` are the GitHub user
/// objects (login + id), and the inner `commit.author` /
/// `commit.committer` carry the git ident (name, email, date).
/// The handler expects
/// `{id, timestamp, message, author: {name,email,username}, committer: {...}}`
/// so we merge the two.
pub fn commits_response_to_delivery(
    t: &RepoTarget,
    commits: &[Value],
) -> Option<WebhookDelivery> {
    if commits.is_empty() {
        return None;
    }
    let synth_commits: Vec<Value> = commits
        .iter()
        .filter_map(|c| {
            let sha = c.get("sha").and_then(Value::as_str)?;
            let commit = c.get("commit")?;
            let inner_author = commit.get("author");
            let inner_committer = commit.get("committer");
            let outer_author = c.get("author");
            let outer_committer = c.get("committer");
            // Webhook handler reads `timestamp` off the commit
            // object; the list endpoint puts it under
            // `commit.author.date` (with `commit.committer.date`
            // as the alternate). Prefer the committer date — that
            // is what GitHub uses on the webhook itself.
            let timestamp = inner_committer
                .and_then(|v| v.get("date"))
                .and_then(Value::as_str)
                .or_else(|| inner_author.and_then(|v| v.get("date")).and_then(Value::as_str))?;
            let message = commit
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("");
            let author = json!({
                "name":  inner_author.and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or(""),
                "email": inner_author.and_then(|v| v.get("email")).and_then(Value::as_str).unwrap_or(""),
                "username": outer_author.and_then(|v| v.get("login")).and_then(Value::as_str),
            });
            let committer = json!({
                "name":  inner_committer.and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or(""),
                "email": inner_committer.and_then(|v| v.get("email")).and_then(Value::as_str).unwrap_or(""),
                "username": outer_committer.and_then(|v| v.get("login")).and_then(Value::as_str),
            });
            Some(json!({
                "id":        sha,
                "timestamp": timestamp,
                "message":   message,
                "author":    author,
                "committer": committer,
            }))
        })
        .collect();
    if synth_commits.is_empty() {
        return None;
    }
    Some(make_delivery(
        "push",
        json!({
            "repository": repository_block(t),
            "commits":    synth_commits,
        }),
    ))
}

/// Shape an `{id, login}` block matching what `upsert_org_from`
/// expects. Used by the org-scoped synth paths (teams / members) —
/// the org-list reconcile loop doesn't ride through `repository`,
/// so we synthesise the same shape the `organization` field on the
/// real webhook event carries.
fn organization_block(org_github_id: i64, org_login: &str) -> Value {
    json!({
        "id":    org_github_id,
        "login": org_login,
    })
}

/// Synthesise one `team` webhook delivery (action `created`) per
/// team returned by `GET /orgs/{org}/teams`.
///
/// `created` is the lowest-impact action the handler accepts —
/// `handle_team` upserts on `(org_id, github_id)` regardless of
/// action, so the value here is "make the handler see a team it
/// hasn't seen, or rename one it has". Renames flow naturally
/// through the same upsert.
pub fn teams_response_to_deliveries(
    org_github_id: i64,
    org_login: &str,
    teams: &[Value],
) -> Vec<WebhookDelivery> {
    let mut out = Vec::with_capacity(teams.len());
    for team in teams {
        // Defensive: skip entries that are missing the fields
        // `handle_team` requires (`id`, `slug`). The handler would
        // otherwise return `MissingField` and the per-tick error
        // count would inflate for no operator-actionable reason.
        if team.get("id").and_then(Value::as_i64).is_none()
            || team.get("slug").and_then(Value::as_str).is_none()
        {
            continue;
        }
        out.push(make_delivery(
            "team",
            json!({
                "action":       "created",
                "organization": organization_block(org_github_id, org_login),
                "team":         team,
            }),
        ));
    }
    out
}

/// Synthesise one `membership` webhook delivery (action `added`)
/// per member returned by `GET /orgs/{org}/members`.
///
/// `handle_membership` upserts a `(user, org)` membership row with
/// `role = Member`. GitHub's org-members list doesn't include role,
/// so every reconciler-sourced membership flat-lines on `Member`;
/// real role changes ride the webhook path and overwrite via
/// upsert. The membership PK is `(user_id, org_id)`, so this is
/// safe.
pub fn members_response_to_deliveries(
    org_github_id: i64,
    org_login: &str,
    members: &[Value],
) -> Vec<WebhookDelivery> {
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        if member.get("id").and_then(Value::as_i64).is_none()
            || member.get("login").and_then(Value::as_str).is_none()
        {
            continue;
        }
        out.push(make_delivery(
            "membership",
            json!({
                "action":       "added",
                "organization": organization_block(org_github_id, org_login),
                "member":       member,
            }),
        ));
    }
    out
}

/// Walk a list-response array picking the maximum timestamp at any
/// of the candidate JSON pointers. Used to advance the cursor's
/// `since` to "newest thing we just observed".
pub fn max_timestamp(items: &[Value], paths: &[&str]) -> Option<DateTime<Utc>> {
    let mut max: Option<DateTime<Utc>> = None;
    for item in items {
        for path in paths {
            // Walk a dotted path through the item.
            let mut cur = item;
            let mut ok = true;
            for seg in path.split('.') {
                match cur.get(seg) {
                    Some(v) => cur = v,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            if let Some(s) = cur.as_str() {
                if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                    let t = t.with_timezone(&Utc);
                    max = Some(max.map_or(t, |m| m.max(t)));
                }
            }
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> RepoTarget {
        RepoTarget {
            org_id: Uuid::new_v4(),
            org_github_id: 42,
            owner_login: "octo".into(),
            repo_id: Uuid::new_v4(),
            repo_github_id: 7,
            repo_name: "hello".into(),
        }
    }

    #[test]
    fn pulls_open_emits_opened() {
        let t = target();
        let ds = pulls_response_to_deliveries(
            &t,
            &[json!({
                "node_id": "PR_1",
                "state":   "open",
                "created_at": "2024-01-01T00:00:00Z",
                "user": { "id": 1, "login": "alice" }
            })],
        );
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].event, "pull_request");
        assert_eq!(ds[0].payload["action"], "opened");
        assert_eq!(ds[0].payload["repository"]["name"], "hello");
        assert!(ds[0].delivery_id.starts_with("recon:"));
    }

    #[test]
    fn pulls_closed_and_merged_emits_merged_true() {
        let t = target();
        let ds = pulls_response_to_deliveries(
            &t,
            &[json!({
                "node_id": "PR_2",
                "state":   "closed",
                "created_at": "2024-01-01T00:00:00Z",
                "closed_at":  "2024-01-02T00:00:00Z",
                "merged_at":  "2024-01-02T00:00:00Z",
                "user": { "id": 1, "login": "alice" }
            })],
        );
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].payload["action"], "closed");
        assert_eq!(ds[0].payload["pull_request"]["merged"], true);
    }

    #[test]
    fn issues_response_skips_pull_requests() {
        let t = target();
        let ds = issues_response_to_deliveries(
            &t,
            &[
                json!({
                    "node_id": "I_1",
                    "state":   "open",
                    "created_at": "2024-01-01T00:00:00Z",
                    "user": { "id": 1, "login": "alice" }
                }),
                json!({
                    "node_id": "PR_pretending_to_be_issue",
                    "state":   "open",
                    "pull_request": { "url": "..." },
                    "user": { "id": 1, "login": "alice" }
                }),
            ],
        );
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].payload["issue"]["node_id"], "I_1");
    }

    #[test]
    fn commits_response_reshapes_into_push() {
        let t = target();
        let d = commits_response_to_delivery(
            &t,
            &[json!({
                "sha": "deadbeef",
                "commit": {
                    "author":    { "name": "Alice", "email": "alice@x", "date": "2024-04-04T04:04:04Z" },
                    "committer": { "name": "Alice", "email": "alice@x", "date": "2024-04-04T04:04:05Z" },
                    "message":   "feat: thing"
                },
                "author":    { "id": 1, "login": "alice" },
                "committer": { "id": 1, "login": "alice" }
            })],
        )
        .expect("delivery");
        assert_eq!(d.event, "push");
        let c0 = &d.payload["commits"][0];
        assert_eq!(c0["id"], "deadbeef");
        assert_eq!(c0["timestamp"], "2024-04-04T04:04:05Z");
        assert_eq!(c0["author"]["username"], "alice");
        assert_eq!(c0["committer"]["username"], "alice");
    }

    #[test]
    fn commits_empty_array_yields_none() {
        assert!(commits_response_to_delivery(&target(), &[]).is_none());
    }

    #[test]
    fn max_timestamp_walks_dotted_paths() {
        let items = vec![
            json!({ "updated_at": "2024-01-01T00:00:00Z" }),
            json!({ "updated_at": "2024-06-06T06:06:06Z" }),
            json!({ "updated_at": "2024-03-03T03:03:03Z" }),
        ];
        let m = max_timestamp(&items, &["updated_at"]).unwrap();
        assert_eq!(m.to_rfc3339(), "2024-06-06T06:06:06+00:00");
    }
}
