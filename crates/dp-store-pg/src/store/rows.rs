
use dp_domain::event::ActivityEvent;
use dp_domain::fetch::{FetchCursor, FetchRun};
use dp_domain::identity::{IdentityLinkPending, UserIdentity, VerifiedVia};
use dp_domain::inbox::{InboxIssueRow, InboxStatus, UserIssueState};
use dp_domain::membership::Membership;
use dp_domain::milestone::{Milestone, MilestoneState};
use dp_domain::org::Org;
use dp_domain::pin::{Pin, PinKind};
use dp_domain::repo::Repo;
use dp_domain::setting::UserSetting;
use dp_domain::issue::{Issue, IssueState, RepoSummary};
use dp_domain::issue_mutation::{IssueMutation, IssueMutationOp, IssueMutationResult};
use dp_domain::event::EventKind;
use dp_domain::tag::Tag;
use dp_domain::tag_link::TagLink;
use dp_domain::board_link::{
    BoardItem, BoardLink,
};
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind};
use dp_domain::project::{
    PortfolioRawRow, Project, ProjectStatus,
};
use dp_domain::project_view::{
    ProjectView, ProjectViewFilterClause, ProjectViewVisibility,
};
use dp_domain::store::{
    EventActorRow, StoreError,
};
use dp_domain::team::Team;
use dp_domain::user::User;
use dp_domain::webhook::WebhookDelivery;
use serde_json::Value as JsonValue;
use sqlx::Row;
use uuid::Uuid;

use crate::encode::{
    actor_role_from_text, event_kind_from_text,
    tag_link_kind_from_text, tag_scope_kind_from_text,
    fetch_run_kind_from_text, membership_role_from_text, resource_kind_from_text,
};

use super::{invalid, map_sqlx};



pub(super) fn project_view_from_row(
    r: &sqlx::postgres::PgRow,
) -> Result<ProjectView, StoreError> {
    let filter_json: serde_json::Value = r.try_get("filter_json").map_err(map_sqlx)?;
    let filter_clauses: Vec<ProjectViewFilterClause> =
        serde_json::from_value(filter_json).map_err(|e| {
            StoreError::Invalid(format!("filter_json decode: {e}"))
        })?;
    let categories_json: serde_json::Value = r.try_get("categories").map_err(map_sqlx)?;
    let categories: Vec<String> =
        serde_json::from_value(categories_json).map_err(|e| {
            StoreError::Invalid(format!("categories decode: {e}"))
        })?;
    let visibility_text: String = r.try_get("visibility").map_err(map_sqlx)?;
    let visibility = ProjectViewVisibility::from_str(&visibility_text)
        .ok_or_else(|| {
            StoreError::Invalid(format!("unknown view visibility: {visibility_text}"))
        })?;
    Ok(ProjectView {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        owner_user_id: r.try_get("owner_user_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        group_by: r.try_get("group_by").map_err(map_sqlx)?,
        filter_clauses,
        sort: r.try_get("sort").map_err(map_sqlx)?,
        position: r.try_get("position").map_err(map_sqlx)?,
        visibility,
        start_date: r.try_get("start_date").map_err(map_sqlx)?,
        due_date: r.try_get("due_date").map_err(map_sqlx)?,
        categories,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_user(r: &sqlx::postgres::PgRow) -> Result<User, StoreError> {
    // The `role` column landed in migration 0047 (DOCS/SCOPE-AUTHZ-USERS.md
    // §2). Older callers that SELECT without `role` get the Reader default
    // via the column's NOT NULL DEFAULT, so missing-column reads would only
    // surface here as a try_get failure — they should be fixed at the call
    // site, not papered over with `unwrap_or`.
    let role_str: String = r.try_get("role").map_err(map_sqlx)?;
    let role = dp_domain::user::Role::from_str(&role_str).ok_or_else(|| {
        StoreError::Invalid(format!("unknown user role: {role_str}"))
    })?;
    Ok(User {
        id: r.try_get("id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        login: r.try_get("login").map_err(map_sqlx)?,
        email: r.try_get("email").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        role,
        deleted_at: r.try_get("deleted_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_user_identity(
    r: &sqlx::postgres::PgRow,
) -> Result<UserIdentity, StoreError> {
    let via_text: String = r.try_get("verified_via").map_err(map_sqlx)?;
    let verified_via = VerifiedVia::from_str(&via_text).ok_or_else(|| {
        StoreError::Invalid(format!("unknown verified_via: {via_text}"))
    })?;
    Ok(UserIdentity {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        github_user_id: r.try_get("github_user_id").map_err(map_sqlx)?,
        github_login: r.try_get("github_login").map_err(map_sqlx)?,
        is_primary: r.try_get("is_primary").map_err(map_sqlx)?,
        linked_at: r.try_get("linked_at").map_err(map_sqlx)?,
        verified_via,
    })
}

pub(super) fn row_to_identity_link_pending(
    r: &sqlx::postgres::PgRow,
) -> Result<IdentityLinkPending, StoreError> {
    Ok(IdentityLinkPending {
        nonce: r.try_get("nonce").map_err(map_sqlx)?,
        dp_user_id: r.try_get("dp_user_id").map_err(map_sqlx)?,
        session_id: r.try_get("session_id").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        expires_at: r.try_get("expires_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_org(r: &sqlx::postgres::PgRow) -> Result<Org, StoreError> {
    Ok(Org {
        id: r.try_get("id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        login: r.try_get("login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_team(r: &sqlx::postgres::PgRow) -> Result<Team, StoreError> {
    Ok(Team {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        slug: r.try_get("slug").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_repo(r: &sqlx::postgres::PgRow) -> Result<Repo, StoreError> {
    Ok(Repo {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_repo_metadata(
    r: &sqlx::postgres::PgRow,
) -> Result<dp_domain::RepoMetadata, StoreError> {
    Ok(dp_domain::RepoMetadata {
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        stars: r.try_get("stars").map_err(map_sqlx)?,
        forks: r.try_get("forks").map_err(map_sqlx)?,
        watchers: r.try_get("watchers").map_err(map_sqlx)?,
        open_issues_remote: r.try_get("open_issues_remote").map_err(map_sqlx)?,
        primary_language: r.try_get("primary_language").map_err(map_sqlx)?,
        default_branch: r.try_get("default_branch").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        homepage: r.try_get("homepage").map_err(map_sqlx)?,
        is_archived: r.try_get("is_archived").map_err(map_sqlx)?,
        is_fork: r.try_get("is_fork").map_err(map_sqlx)?,
        is_private: r.try_get("is_private").map_err(map_sqlx)?,
        pushed_at: r.try_get("pushed_at").map_err(map_sqlx)?,
        metadata_updated_at: r.try_get("metadata_updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_repo_summary(r: &sqlx::postgres::PgRow) -> Result<RepoSummary, StoreError> {
    Ok(RepoSummary {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        org_login: r.try_get("org_login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        open_issue_count: r.try_get("open_issue_count").map_err(map_sqlx)?,
        last_activity_at: r.try_get("last_activity_at").map_err(map_sqlx)?,
    })
}

/// Build a JSONB containment array for the `labels` / `assignees`
/// AND filter (`IssueListFilter::labels` / `::assignees`). Returns
/// `None` for an empty input so the bind site can pass `NULL` and
/// the SQL guard (`$N::jsonb IS NULL OR …`) skips the containment
/// check. Non-empty inputs serialise to `JsonValue::Array(strings)`
/// so the PG side can use `column @> $N::jsonb`.
pub(super) fn labels_or_assignees_json(values: &[String]) -> Option<JsonValue> {
    if values.is_empty() {
        return None;
    }
    Some(JsonValue::Array(
        values
            .iter()
            .map(|s| JsonValue::String(s.clone()))
            .collect(),
    ))
}

/// One-line `payload_summary` for issue-timeline rows. The text
/// is intentionally compact — the frontend prepends an icon /
/// actor list, so we just describe the change. Falls back to the
/// kind label if the payload doesn't carry an obvious summary
/// field.
pub(super) fn summarise_timeline_payload(kind: EventKind, payload: &JsonValue) -> String {
    match kind {
        EventKind::IssueOpened => "opened the issue".to_string(),
        EventKind::IssueClosed => match payload.get("state_reason").and_then(|v| v.as_str()) {
            Some("not_planned") => "closed as not planned".to_string(),
            Some("completed") => "closed as completed".to_string(),
            _ => "closed the issue".to_string(),
        },
        EventKind::IssueComment => {
            let body = payload
                .get("body")
                .and_then(|v| v.as_str())
                .or_else(|| payload.get("body_excerpt").and_then(|v| v.as_str()))
                .unwrap_or("");
            let trimmed: String = body.chars().take(120).collect();
            if trimmed.is_empty() {
                "commented".to_string()
            } else {
                format!("commented: {trimmed}")
            }
        }
        _ => format!("{:?}", kind),
    }
}

pub(super) fn row_to_issue(r: &sqlx::postgres::PgRow) -> Result<Issue, StoreError> {
    let state_text: String = r.try_get("state").map_err(map_sqlx)?;
    let state = IssueState::from_str(&state_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown issue state: {state_text}")))?;
    let labels_json: JsonValue = r.try_get("labels").map_err(map_sqlx)?;
    let assignees_json: JsonValue = r.try_get("assignees").map_err(map_sqlx)?;
    let labels: Vec<String> = serde_json::from_value(labels_json)
        .map_err(|e| StoreError::Invalid(format!("labels not a string array: {e}")))?;
    let assignees: Vec<String> = serde_json::from_value(assignees_json)
        .map_err(|e| StoreError::Invalid(format!("assignees not a string array: {e}")))?;
    Ok(Issue {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        github_id: r.try_get("github_id").map_err(map_sqlx)?,
        number: r.try_get("number").map_err(map_sqlx)?,
        title: r.try_get("title").map_err(map_sqlx)?,
        body: r.try_get("body").map_err(map_sqlx)?,
        state,
        labels,
        assignees,
        milestone: r.try_get("milestone").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        github_node_id: r.try_get("github_node_id").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        // Tolerate SELECT lists that pre-date the 0041 migration —
        // they simply default to non-local, which is correct for
        // every row created before the feature shipped.
        is_local: r.try_get("is_local").unwrap_or(false),
    })
}

pub(super) fn row_to_user_issue_state(
    r: &sqlx::postgres::PgRow,
) -> Result<UserIssueState, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = InboxStatus::from_str(&status_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown inbox status: {status_text}")))?;
    Ok(UserIssueState {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        last_seen_version: r.try_get("last_seen_version").map_err(map_sqlx)?,
        status,
        snoozed_until: r.try_get("snoozed_until").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_inbox_issue_row(
    r: &sqlx::postgres::PgRow,
) -> Result<InboxIssueRow, StoreError> {
    let issue = row_to_issue(r)?;
    let last_seen_version: i64 = r.try_get("last_seen_version").map_err(map_sqlx)?;
    let status_text: String = r.try_get("inbox_status").map_err(map_sqlx)?;
    let status = InboxStatus::from_str(&status_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown inbox status: {status_text}")))?;
    let snoozed_until = r.try_get("snoozed_until").map_err(map_sqlx)?;
    Ok(InboxIssueRow {
        unread: issue.version > last_seen_version,
        issue,
        status,
        snoozed_until,
    })
}

pub(super) fn row_to_membership(r: &sqlx::postgres::PgRow) -> Result<Membership, StoreError> {
    let role_text: String = r.try_get("role").map_err(map_sqlx)?;
    Ok(Membership {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        role: membership_role_from_text(&role_text),
        home_org: r.try_get("home_org").map_err(map_sqlx)?,
        joined_at: r.try_get("joined_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_activity_event(r: &sqlx::postgres::PgRow) -> Result<ActivityEvent, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = event_kind_from_text(&kind_text).map_err(invalid)?;
    Ok(ActivityEvent {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind,
        ts: r.try_get("ts").map_err(map_sqlx)?,
        external_id: r.try_get("external_id").map_err(map_sqlx)?,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_fetch_run(r: &sqlx::postgres::PgRow) -> Result<FetchRun, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = fetch_run_kind_from_text(&kind_text).map_err(invalid)?;
    Ok(FetchRun {
        id: r.try_get("id").map_err(map_sqlx)?,
        kind,
        started: r.try_get("started").map_err(map_sqlx)?,
        finished: r.try_get("finished").map_err(map_sqlx)?,
        items: r.try_get("items").map_err(map_sqlx)?,
        errors: r.try_get("errors").map_err(map_sqlx)?,
        partial: r.try_get("partial").map_err(map_sqlx)?,
        error_sample: r
            .try_get::<Option<JsonValue>, _>("error_sample")
            .map_err(map_sqlx)?
            .map(|v| serde_json::from_value(v).map_err(|e| invalid(e.to_string())))
            .transpose()?,
    })
}

pub(super) fn row_to_fetch_cursor(r: &sqlx::postgres::PgRow) -> Result<FetchCursor, StoreError> {
    let rk_text: String = r.try_get("resource_kind").map_err(map_sqlx)?;
    let resource_kind = resource_kind_from_text(&rk_text).map_err(invalid)?;
    Ok(FetchCursor {
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        resource_kind,
        since: r.try_get("since").map_err(map_sqlx)?,
        etag: r.try_get("etag").map_err(map_sqlx)?,
        last_event_id: r.try_get("last_event_id").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_webhook_delivery(r: &sqlx::postgres::PgRow) -> Result<WebhookDelivery, StoreError> {
    Ok(WebhookDelivery {
        id: r.try_get("id").map_err(map_sqlx)?,
        delivery_id: r.try_get("delivery_id").map_err(map_sqlx)?,
        event: r.try_get("event").map_err(map_sqlx)?,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
        received_at: r.try_get("received_at").map_err(map_sqlx)?,
        processed_at: r.try_get("processed_at").map_err(map_sqlx)?,
        error: r.try_get("error").map_err(map_sqlx)?,
    })
}

pub(super) fn pin_kind_from_text(s: &str) -> Result<PinKind, StoreError> {
    match s {
        "repo" => Ok(PinKind::Repo),
        "tag" => Ok(PinKind::Tag),
        other => Err(invalid(format!("unknown pin kind {other:?}"))),
    }
}

pub(super) fn row_to_pin(r: &sqlx::postgres::PgRow) -> Result<Pin, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(Pin {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        kind: pin_kind_from_text(&kind_text)?,
        target_id: r.try_get("target_id").map_err(map_sqlx)?,
        position: r.try_get("position").map_err(map_sqlx)?,
        pinned_at: r.try_get("pinned_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_user_setting(r: &sqlx::postgres::PgRow) -> Result<UserSetting, StoreError> {
    Ok(UserSetting {
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        key: r.try_get("key").map_err(map_sqlx)?,
        value: r.try_get("value").map_err(map_sqlx)?,
        is_secret: r.try_get("is_secret").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_event_actor_row(r: &sqlx::postgres::PgRow) -> Result<EventActorRow, StoreError> {
    let role_text: String = r.try_get("role").map_err(map_sqlx)?;
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(EventActorRow {
        event_id: r.try_get("event_id").map_err(map_sqlx)?,
        user_id: r.try_get("user_id").map_err(map_sqlx)?,
        role: actor_role_from_text(&role_text).map_err(invalid)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind: event_kind_from_text(&kind_text).map_err(invalid)?,
        ts: r.try_get("ts").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_tag(r: &sqlx::postgres::PgRow) -> Result<Tag, StoreError> {
    let scope_text: String = r.try_get("scope_kind").map_err(map_sqlx)?;
    Ok(Tag {
        id: r.try_get("id").map_err(map_sqlx)?,
        scope_kind: tag_scope_kind_from_text(&scope_text).map_err(invalid)?,
        scope_user_id: r.try_get("scope_user_id").map_err(map_sqlx)?,
        scope_team_id: r.try_get("scope_team_id").map_err(map_sqlx)?,
        scope_org_id: r.try_get("scope_org_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        color: r.try_get("color").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        archived_at: r.try_get("archived_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_tag_link(r: &sqlx::postgres::PgRow) -> Result<TagLink, StoreError> {
    let kind_text: String = r.try_get("kind").map_err(map_sqlx)?;
    Ok(TagLink {
        id: r.try_get("id").map_err(map_sqlx)?,
        tag_id: r.try_get("tag_id").map_err(map_sqlx)?,
        kind: tag_link_kind_from_text(&kind_text).map_err(invalid)?,
        target_repo_id: r.try_get("target_repo_id").map_err(map_sqlx)?,
        target_issue_id: r.try_get("target_issue_id").map_err(map_sqlx)?,
        target_user_id: r.try_get("target_user_id").map_err(map_sqlx)?,
        target_team_id: r.try_get("target_team_id").map_err(map_sqlx)?,
        added_by: r.try_get("added_by").map_err(map_sqlx)?,
        added_at: r.try_get("added_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_milestone(r: &sqlx::postgres::PgRow) -> Result<Milestone, StoreError> {
    let state_text: String = r.try_get("state").map_err(map_sqlx)?;
    let state = MilestoneState::from_str(&state_text)
        .ok_or_else(|| StoreError::Invalid(format!("unknown milestone state {state_text:?}")))?;
    Ok(Milestone {
        id: r.try_get("id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        github_number: r.try_get("github_number").map_err(map_sqlx)?,
        github_node_id: r.try_get("github_node_id").map_err(map_sqlx)?,
        title: r.try_get("title").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        state,
        due_on: r.try_get("due_on").map_err(map_sqlx)?,
        open_issues: r.try_get("open_issues").map_err(map_sqlx)?,
        closed_issues: r.try_get("closed_issues").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        closed_at: r.try_get("closed_at").map_err(map_sqlx)?,
        fetched_at: r.try_get("fetched_at").map_err(map_sqlx)?,
        remote_missing_streak: r.try_get("remote_missing_streak").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_project(r: &sqlx::postgres::PgRow) -> Result<Project, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = ProjectStatus::from_str(&status_text)
        .ok_or_else(|| invalid(format!("unknown project status: {status_text}")))?;
    Ok(Project {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        description: r.try_get("description").map_err(map_sqlx)?,
        lead_user_id: r.try_get("lead_user_id").map_err(map_sqlx)?,
        status,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        issue_count: r.try_get("issue_count").map_err(map_sqlx)?,
        closed_issue_count: r.try_get("closed_issue_count").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        primary_milestone_id: r.try_get("primary_milestone_id").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_portfolio_raw(r: &sqlx::postgres::PgRow) -> Result<PortfolioRawRow, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = ProjectStatus::from_str(&status_text)
        .ok_or_else(|| invalid(format!("unknown project status: {status_text}")))?;
    let lead_id: Option<Uuid> = r.try_get("lead_user_id").map_err(map_sqlx)?;
    let lead_login: Option<String> = r.try_get("lead_login").map_err(map_sqlx)?;
    let lead = match (lead_id, lead_login) {
        (Some(id), Some(login)) => Some((id, login)),
        _ => None,
    };
    Ok(PortfolioRawRow {
        id: r.try_get("id").map_err(map_sqlx)?,
        org_id: r.try_get("org_id").map_err(map_sqlx)?,
        org_login: r.try_get("org_login").map_err(map_sqlx)?,
        name: r.try_get("name").map_err(map_sqlx)?,
        status,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        issue_count: r.try_get("issue_count").map_err(map_sqlx)?,
        closed_issue_count: r.try_get("closed_issue_count").map_err(map_sqlx)?,
        progress_pct: r.try_get("progress_pct").map_err(map_sqlx)?,
        slip_days: r.try_get("slip_days").map_err(map_sqlx)?,
        issue_overdue_count: r.try_get("issue_overdue_count").map_err(map_sqlx)?,
        lead,
        mirrored_to_github: r.try_get("mirrored_to_github").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        total: r.try_get("total").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_board_link(r: &sqlx::postgres::PgRow) -> Result<BoardLink, StoreError> {
    Ok(BoardLink {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        github_board_node_id: r.try_get("github_board_node_id").map_err(map_sqlx)?,
        github_board_title: r.try_get("github_board_title").map_err(map_sqlx)?,
        github_board_url: r.try_get("github_board_url").map_err(map_sqlx)?,
        github_board_cached_at: r.try_get("github_board_cached_at").map_err(map_sqlx)?,
        start_field_node_id: r.try_get("start_field_node_id").map_err(map_sqlx)?,
        due_field_node_id: r.try_get("due_field_node_id").map_err(map_sqlx)?,
        status_field_node_id: r.try_get("status_field_node_id").map_err(map_sqlx)?,
        last_mirror_at: r.try_get("last_mirror_at").map_err(map_sqlx)?,
        last_mirror_error: r.try_get("last_mirror_error").map_err(map_sqlx)?,
        created_by: r.try_get("created_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_board_item(r: &sqlx::postgres::PgRow) -> Result<BoardItem, StoreError> {
    Ok(BoardItem {
        link_id: r.try_get("link_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        item_node_id: r.try_get("item_node_id").map_err(map_sqlx)?,
        last_synced_at: r.try_get("last_synced_at").map_err(map_sqlx)?,
        last_error: r.try_get("last_error").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_issue_dates(r: &sqlx::postgres::PgRow) -> Result<IssueDates, StoreError> {
    Ok(IssueDates {
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        start_at: r.try_get("start_at").map_err(map_sqlx)?,
        due_at: r.try_get("due_at").map_err(map_sqlx)?,
        mirror_node_id: r.try_get("mirror_node_id").map_err(map_sqlx)?,
        mirror_synced_at: r.try_get("mirror_synced_at").map_err(map_sqlx)?,
        mirror_error: r.try_get("mirror_error").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

pub(super) fn row_to_projectv2_mirror_task(
    r: &sqlx::postgres::PgRow,
) -> Result<ProjectV2MirrorTask, StoreError> {
    let kind_s: String = r.try_get("kind").map_err(map_sqlx)?;
    let kind = match kind_s.as_str() {
        "mirror_dates" => ProjectV2MirrorTaskKind::MirrorDates,
        "pull_back" => ProjectV2MirrorTaskKind::PullBack,
        other => return Err(invalid(format!("unknown mirror task kind: {other}"))),
    };
    Ok(ProjectV2MirrorTask {
        id: r.try_get("id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        kind,
        payload: r.try_get::<JsonValue, _>("payload").map_err(map_sqlx)?,
        attempts: r.try_get("attempts").map_err(map_sqlx)?,
        last_error: r.try_get("last_error").map_err(map_sqlx)?,
        enqueued_at: r.try_get("enqueued_at").map_err(map_sqlx)?,
        processed_at: r.try_get("processed_at").map_err(map_sqlx)?,
    })
}

pub(super) fn issue_mutation_op_to_text(op: IssueMutationOp) -> &'static str {
    match op {
        IssueMutationOp::Create => "create",
        IssueMutationOp::Update => "update",
        IssueMutationOp::Close => "close",
        IssueMutationOp::Reopen => "reopen",
        IssueMutationOp::Comment => "comment",
    }
}

pub(super) fn issue_mutation_op_from_text(s: &str) -> Result<IssueMutationOp, StoreError> {
    match s {
        "create" => Ok(IssueMutationOp::Create),
        "update" => Ok(IssueMutationOp::Update),
        "close" => Ok(IssueMutationOp::Close),
        "reopen" => Ok(IssueMutationOp::Reopen),
        "comment" => Ok(IssueMutationOp::Comment),
        other => Err(invalid(format!("unknown issue mutation op: {other}"))),
    }
}

pub(super) fn issue_mutation_result_to_text(r: IssueMutationResult) -> &'static str {
    match r {
        IssueMutationResult::Pending => "pending",
        IssueMutationResult::Committed => "committed",
        IssueMutationResult::Failed => "failed",
        IssueMutationResult::PendingRemoteTimeout => "pending_remote_timeout",
    }
}

pub(super) fn issue_mutation_result_from_text(s: &str) -> Result<IssueMutationResult, StoreError> {
    match s {
        "pending" => Ok(IssueMutationResult::Pending),
        "committed" => Ok(IssueMutationResult::Committed),
        "failed" => Ok(IssueMutationResult::Failed),
        "pending_remote_timeout" => Ok(IssueMutationResult::PendingRemoteTimeout),
        other => Err(invalid(format!("unknown issue mutation result: {other}"))),
    }
}

pub(super) fn row_to_issue_mutation(r: &sqlx::postgres::PgRow) -> Result<IssueMutation, StoreError> {
    let op_s: String = r.try_get("op").map_err(map_sqlx)?;
    let result_s: String = r.try_get("result").map_err(map_sqlx)?;
    Ok(IssueMutation {
        id: r.try_get("id").map_err(map_sqlx)?,
        actor_user_id: r.try_get("actor_user_id").map_err(map_sqlx)?,
        issue_id: r.try_get("issue_id").map_err(map_sqlx)?,
        repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
        op: issue_mutation_op_from_text(&op_s)?,
        version_before: r.try_get("version_before").map_err(map_sqlx)?,
        version_after: r.try_get("version_after").map_err(map_sqlx)?,
        diff: r.try_get::<JsonValue, _>("diff").map_err(map_sqlx)?,
        result: issue_mutation_result_from_text(&result_s)?,
        github_delivery_id: r.try_get("github_delivery_id").map_err(map_sqlx)?,
        error: r.try_get("error").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        finished_at: r.try_get("finished_at").map_err(map_sqlx)?,
    })
}
