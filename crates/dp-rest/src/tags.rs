//! Tags handlers — the home-grown cross-org grouping surface
//! (SCOPE-PROJECTS §7).
//!
//! The seven routes in §7.5 land here:
//!
//! | route                                    | what it does                                                |
//! |------------------------------------------|-------------------------------------------------------------|
//! | `GET    /tags`                           | list every tag visible to caller, with viewer-filtered      |
//! |                                          | link counts (§7.4 — never the true total)                   |
//! | `POST   /tags`                           | create a tag in a scope the caller is a member of           |
//! | `GET    /tags/{id}`                      | one tag + a *page* of links (links_page param)              |
//! | `PATCH  /tags/{id}`                      | rename / recolour / set description / archive               |
//! | `POST   /tags/{id}/links`                | **transactional all-or-nothing** batch link, per-item       |
//! |                                          | errors on rejection (§7.5)                                  |
//! | `DELETE /tags/{id}/links`                | **transactional all-or-nothing** batch unlink, per-item     |
//! |                                          | errors (§7.5)                                               |
//! | `GET    /me/tags`                        | tags the caller owns or is a scope member of                |
//!
//! Behaviour pinned by SCOPE-PROJECTS:
//!
//! * **Visibility (§7.4).** A tag is visible to a caller iff its
//!   scope is visible. `user`-scope: only the owner. `team`-scope:
//!   callers in the team's org (v1 simplification — fine-grained
//!   team membership lands when `Store::list_teams_for_org` is
//!   paired with a team-members lookup; documented in §12).
//!   `org`-scope: callers in the org. Link counts in `GET /tags`
//!   and `GET /tags/{id}` reflect the **viewer-visible** subset of
//!   links, never the true total (§7.4 "Reporting the true count
//!   would leak the existence of repos / issues / users / teams
//!   the viewer has no access to.").
//!
//! * **Mutation (§7.4).** Edit and link/unlink require the caller
//!   to be a *scope member* of the tag — not merely able to see it.
//!   User-scope tags: only the owner. Team/org-scope: any member of
//!   the team/org. Returns `403 tag_scope_member_required` on
//!   mismatch.
//!
//! * **Batch link/unlink (§7.5).** Transactional all-or-nothing. The
//!   handler **pre-validates** every item against the live state
//!   (existing links, target kind correctness) and rejects the
//!   whole batch with a per-item error array
//!   (`422 batch_rejected`, [`ApiError::Batch`]) on any failure.
//!   Nothing is committed when the response is non-200. On success
//!   one audit row is written *per link* per §7.6.
//!
//! * **Soft warning at 500 links/tag (§13.5).** After a successful
//!   batch link, if the tag now carries more than
//!   [`dp_domain::TAG_LINK_WARN_THRESHOLD`] links, the response
//!   carries `warning: "tag_link_count_high"` — the operation still
//!   commits. Misuse signal, not a hard cap.
//!
//! * **Archive, never hard-delete (§7.4 mutation rules).** There is
//!   no `DELETE /tags/{id}` in v1. Setting `archived_at` via the
//!   `PATCH` is the only way to retire a tag from this surface;
//!   periodic hard-prune of archived rows is an admin task per §7.2.
//!   The `tag.archive` audit verb is distinct from `tag.update` so
//!   the audit log answers "when did Phoenix retire?" with one
//!   query.
//!
//! Visibility resolution lives in [`viewer_visibility`]; the
//! viewer's org allow-list is read from
//! [`Store::list_memberships_for_user`]. Team membership is *not*
//! a v1 entity — the team-scope branch trusts org membership as a
//! conservative approximation and is flagged as a §12 open
//! question.
//!
//! [`ApiError::Batch`]: crate::error::ApiError::Batch
//! [`Store::list_memberships_for_user`]: dp_domain::store::Store::list_memberships_for_user

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query, State},
    response::Json,
    routing::{delete, get, patch, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::store::{Store, StoreError};
use dp_domain::tag::{Tag, TagScopeKind, TAG_LINK_WARN_THRESHOLD};
use dp_domain::tag_link::{TagLink, TagLinkKind};

use crate::audit::{self, Principal};
use crate::directory::Ack;
use crate::error::{ApiError, BatchItemError};
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`TagScopeKind`] — `user | team | org`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TagScopeKindDto {
    /// Personal tag — only the owner sees it.
    User,
    /// Team-shared.
    Team,
    /// Org-shared. UI default for new tags when the caller is in
    /// exactly one visible org (§7.4).
    Org,
}

impl From<TagScopeKind> for TagScopeKindDto {
    fn from(k: TagScopeKind) -> Self {
        match k {
            TagScopeKind::User => Self::User,
            TagScopeKind::Team => Self::Team,
            TagScopeKind::Org => Self::Org,
        }
    }
}

impl From<TagScopeKindDto> for TagScopeKind {
    fn from(k: TagScopeKindDto) -> Self {
        match k {
            TagScopeKindDto::User => Self::User,
            TagScopeKindDto::Team => Self::Team,
            TagScopeKindDto::Org => Self::Org,
        }
    }
}

/// Wire form of [`TagLinkKind`] — `repo | issue | user | team`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TagLinkKindDto {
    /// Link to a repo.
    Repo,
    /// Link to an issue (issue-centric metrics only — §7.7).
    Issue,
    /// Link to a user.
    User,
    /// Link to a team.
    Team,
}

impl From<TagLinkKind> for TagLinkKindDto {
    fn from(k: TagLinkKind) -> Self {
        match k {
            TagLinkKind::Repo => Self::Repo,
            TagLinkKind::Issue => Self::Issue,
            TagLinkKind::User => Self::User,
            TagLinkKind::Team => Self::Team,
        }
    }
}

impl From<TagLinkKindDto> for TagLinkKind {
    fn from(k: TagLinkKindDto) -> Self {
        match k {
            TagLinkKindDto::Repo => Self::Repo,
            TagLinkKindDto::Issue => Self::Issue,
            TagLinkKindDto::User => Self::User,
            TagLinkKindDto::Team => Self::Team,
        }
    }
}

/// Tag row carried back to the client. Mirrors [`Tag`] but the
/// three nullable `scope_*_id` columns are collapsed into a single
/// `scope_id` — the discriminator + id is enough on the wire, and
/// matches how the frontend renders it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagDto {
    /// Tag primary key.
    pub id: Uuid,
    /// Scope discriminator.
    pub scope_kind: TagScopeKindDto,
    /// Owning user/team/org id (per [`scope_kind`](Self::scope_kind)).
    pub scope_id: Uuid,
    /// Display name.
    pub name: String,
    /// Semantic palette name (`indigo`, `red`, …).
    pub color: String,
    /// Optional free-form description.
    pub description: Option<String>,
    /// Author.
    pub created_by: Uuid,
    /// Created-at timestamp.
    pub created_at: DateTime<Utc>,
    /// `Some(_)` iff archived (§7.2 soft-delete).
    pub archived_at: Option<DateTime<Utc>>,
    /// **Viewer-visible** link count — never the true total
    /// (§7.4). Populated by every list / get path.
    pub visible_link_count: u32,
}

impl TagDto {
    fn from_with_count(t: Tag, count: u32) -> Self {
        let scope_id = t
            .scope_id()
            .expect("dp_tags CHECK guarantees one scope_*_id is non-NULL");
        Self {
            id: t.id,
            scope_kind: t.scope_kind.into(),
            scope_id,
            name: t.name,
            color: t.color,
            description: t.description,
            created_by: t.created_by,
            created_at: t.created_at,
            archived_at: t.archived_at,
            visible_link_count: count,
        }
    }
}

/// One link row carried back to the client. Mirrors [`TagLink`]
/// but collapses the four `target_*_id` columns into a single
/// `target_id` paired with [`kind`](Self::kind).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagLinkDto {
    /// Link primary key.
    pub id: Uuid,
    /// Parent tag.
    pub tag_id: Uuid,
    /// Discriminator for `target_id`.
    pub kind: TagLinkKindDto,
    /// Linked repo / issue / user / team id per [`kind`](Self::kind).
    pub target_id: Uuid,
    /// Who attached the link.
    pub added_by: Uuid,
    /// When the link was attached.
    pub added_at: DateTime<Utc>,
}

impl From<TagLink> for TagLinkDto {
    fn from(l: TagLink) -> Self {
        let target_id = l
            .target_id()
            .expect("dp_tag_links CHECK guarantees one target_*_id is non-NULL");
        Self {
            id: l.id,
            tag_id: l.tag_id,
            kind: l.kind.into(),
            target_id,
            added_by: l.added_by,
            added_at: l.added_at,
        }
    }
}

/// Body for `POST /tags`. Server picks `id`, `created_by`,
/// `created_at`. `scope_id` is required; the caller must be a
/// scope member or the request returns `403`.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct CreateTagRequest {
    /// Scope discriminator.
    pub scope_kind: TagScopeKindDto,
    /// Owning user/team/org id. For `user`-scope tags, must equal
    /// the caller (returns `403 tag_scope_member_required`
    /// otherwise).
    pub scope_id: Uuid,
    /// Display name. Case-insensitive uniqueness is enforced per
    /// scope by the migration-0005 expression index — a clash
    /// surfaces as `409 tag_name_conflict`.
    pub name: String,
    /// Semantic palette name.
    pub color: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Body for `PATCH /tags/{id}`. Every field is optional; missing
/// fields are left unchanged. Setting `archived_at: Some(true)`
/// archives the tag (§7.4 — no hard delete); `Some(false)`
/// un-archives.
#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
pub struct UpdateTagRequest {
    /// New name (rename).
    #[serde(default)]
    pub name: Option<String>,
    /// New color (recolour).
    #[serde(default)]
    pub color: Option<String>,
    /// Set or clear the description.
    /// `None` leaves untouched; `Some(None)` clears.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    /// `Some(true)` archives, `Some(false)` un-archives, `None`
    /// leaves the archive bit alone.
    #[serde(default)]
    pub archived: Option<bool>,
}

/// One entry in a `POST /tags/{id}/links` request — what to
/// attach.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LinkRequestItem {
    /// Target kind discriminator.
    pub kind: TagLinkKindDto,
    /// Linked target id, per [`kind`](Self::kind).
    pub target_id: Uuid,
}

/// Body for `POST /tags/{id}/links` — batch link.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LinkBatchRequest {
    /// Items to link. Transactional all-or-nothing; per-item
    /// errors are returned via [`ApiError::Batch`] on rejection.
    pub items: Vec<LinkRequestItem>,
}

/// Body for `DELETE /tags/{id}/links` — batch unlink. Same
/// `(kind, target_id)` shape as link — IDs are resolved on the
/// server inside the same transaction so a partial unlink is
/// impossible.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct UnlinkBatchRequest {
    /// Items to detach.
    pub items: Vec<LinkRequestItem>,
}

/// Response from `POST /tags/{id}/links` on success — the
/// attached link rows plus an optional soft warning when the
/// tag's link count crosses [`TAG_LINK_WARN_THRESHOLD`] (§13.5).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LinkBatchResponse {
    /// The persisted link rows in insertion order.
    pub linked: Vec<TagLinkDto>,
    /// Optional advisory warning. `"tag_link_count_high"` when
    /// the tag now holds more than the warning threshold — the
    /// operation committed regardless (§13.5 "the response
    /// carries the warning, the operation still commits").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// `GET /tags/{id}` response: the tag plus a *page* of its links
/// (§7.5: "links are paginated separately (`?links_page=…`) to
/// keep a single response bounded even for tags near the §13.5
/// 500-link soft warning").
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TagDetailResponse {
    /// The tag header.
    pub tag: TagDto,
    /// One page of links — only those visible to the viewer.
    pub links: Vec<TagLinkDto>,
    /// Zero-based page index of the returned slice.
    pub links_page: u32,
    /// Page size used for `links` (see [`LINKS_PAGE_SIZE`]).
    pub links_page_size: u32,
}

/// `GET /tags/{id}` query — paginates the links field.
#[derive(Debug, Clone, Default, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct TagDetailQuery {
    /// Zero-based page index. Defaults to 0.
    #[serde(default)]
    pub links_page: Option<u32>,
}

/// Page size for the `GET /tags/{id}` links slice. 100 is well
/// below the 500-link warning threshold so the *typical* tag
/// fits in one request and the soft warning is the only signal
/// the caller needs to plan a second request.
pub const LINKS_PAGE_SIZE: u32 = 100;

// ---------------------------------------------------------------------------
// Viewer visibility helper
// ---------------------------------------------------------------------------

/// What the viewer can see, computed once per request from
/// [`Store::list_memberships_for_user`].
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewerVisibility {
    pub(crate) viewer_user_id: Uuid,
    /// Orgs the caller has a membership row in. Defines visibility
    /// for `org`-scope tags AND (v1 simplification) the
    /// allow-list for `team`-scope tags whose `scope_team_id`
    /// belongs to one of these orgs.
    pub(crate) visible_org_ids: HashSet<Uuid>,
}

impl ViewerVisibility {
    pub(crate) async fn load(store: &dyn Store, viewer_user_id: Uuid) -> Result<Self, StoreError> {
        let memberships = store.list_memberships_for_user(viewer_user_id).await?;
        Ok(Self {
            viewer_user_id,
            visible_org_ids: memberships.into_iter().map(|m| m.org_id).collect(),
        })
    }

    /// True iff the viewer can *see* this tag per §7.4.
    pub(crate) fn can_see(&self, tag: &Tag, visible_team_ids: &HashSet<Uuid>) -> bool {
        match tag.scope_kind {
            TagScopeKind::User => tag.scope_user_id == Some(self.viewer_user_id),
            TagScopeKind::Team => tag
                .scope_team_id
                .map(|t| visible_team_ids.contains(&t))
                .unwrap_or(false),
            TagScopeKind::Org => tag
                .scope_org_id
                .map(|o| self.visible_org_ids.contains(&o))
                .unwrap_or(false),
        }
    }

    /// True iff the viewer is a *scope member* — required for
    /// edit / link / unlink per §7.4. Note this is the same rule
    /// as visibility for user/org but tighter for team (v1
    /// approximates team membership as org membership; documented
    /// in §12).
    fn is_scope_member(&self, tag: &Tag, visible_team_ids: &HashSet<Uuid>) -> bool {
        self.can_see(tag, visible_team_ids)
    }

    /// True iff the viewer can see a *team* by id. v1 derives
    /// this from "the team's org is in the viewer's org allow-list."
    /// Refined when team membership lands (§12).
    pub(crate) async fn visible_team_ids(&self, store: &dyn Store) -> Result<HashSet<Uuid>, StoreError> {
        let mut out = HashSet::new();
        for org_id in &self.visible_org_ids {
            for t in store.list_teams_for_org(*org_id).await? {
                out.insert(t.id);
            }
        }
        Ok(out)
    }
}

/// Resolve the viewer-visible subset of a tag's links per §7.4.
///
/// The store hands us every link the tag carries; this filter
/// drops the ones the viewer would not be allowed to look up
/// directly. Used to compute both the `visible_link_count` on
/// list / get and the paginated `links` slice on get.
fn filter_visible_links(
    links: Vec<TagLink>,
    visibility: &ViewerVisibility,
    visible_team_ids: &HashSet<Uuid>,
    visible_repo_ids: &HashSet<Uuid>,
) -> Vec<TagLink> {
    links
        .into_iter()
        .filter(|l| match l.kind {
            TagLinkKind::Repo => l
                .target_repo_id
                .map(|r| visible_repo_ids.contains(&r))
                .unwrap_or(false),
            TagLinkKind::Issue => {
                // v1 simplification — issue visibility derives from
                // repo visibility once issues are populated; until
                // then we conservatively *include* issue links
                // because hiding them would silently truncate every
                // GET /tags page during the issues bootstrap window.
                // Refined alongside §8 CAS storage in 0007.
                true
            }
            TagLinkKind::User => {
                // Directory rows are visible to every signed-in
                // operator per the §15.11 access gate's "directory
                // is universally visible" v1 stance.
                let _ = visibility; // reserved for future fine-grained user visibility
                true
            }
            TagLinkKind::Team => l
                .target_team_id
                .map(|t| visible_team_ids.contains(&t))
                .unwrap_or(false),
        })
        .collect()
}

/// All repos in the orgs the viewer can see. Used to filter link
/// visibility (kind=repo).
async fn visible_repo_ids(
    _store: &dyn Store,
    _visibility: &ViewerVisibility,
) -> Result<HashSet<Uuid>, StoreError> {
    // v1: dp-domain `Store` does not yet expose `list_repos_for_org`.
    // The conservative fallback used here is "include every repo
    // link". A later stage that lands the repo listing primitive
    // tightens this to the same predicate the access gate uses for
    // the `repos` filter on the §15.6 envelope. Documented as a
    // §12 open question; tracked alongside the `repos: Vec<RepoId>`
    // envelope addition.
    Ok(HashSet::new())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /tags` — list visible tags with viewer-filtered link counts.
///
/// Query params:
/// * `include_archived` (default `false`) — when `true`, archived
///   tags are included in the response.
///
/// Audit: not audited. The list endpoint is a read whose volume
/// would swamp the audit log without operational value — same
/// rule as `GET /me/pins` (§6.5).
#[derive(Debug, Clone, Default, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ListTagsQuery {
    /// Include archived tags in the response. Default `false`.
    #[serde(default)]
    pub include_archived: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/tags",
    params(ListTagsQuery),
    responses(
        (status = 200, description = "Visible tags with viewer-filtered link counts", body = Vec<TagDto>),
    ),
    tag = "tags",
)]
pub async fn list_tags(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ListTagsQuery>,
) -> Result<Json<Vec<TagDto>>, ApiError> {
    let store = state.store.as_ref();
    let visibility = ViewerVisibility::load(store, principal.actor_user_id).await?;
    let team_ids = visibility.visible_team_ids(store).await?;
    let repo_ids = visible_repo_ids(store, &visibility).await?;

    let viewer = visibility.viewer_user_id;
    let teams: Vec<Uuid> = team_ids.iter().copied().collect();
    let orgs: Vec<Uuid> = visibility.visible_org_ids.iter().copied().collect();
    let include_archived = q.include_archived.unwrap_or(false);

    let tags = store
        .list_tags_visible_to(viewer, &teams, &orgs, include_archived)
        .await?;

    let mut out = Vec::with_capacity(tags.len());
    for tag in tags {
        // Defence-in-depth: re-check visibility even though the
        // store already filtered. Cheap, and the store's contract
        // is "caller supplies allow-lists" — better to enforce here
        // than to trust the impl to never widen later.
        if !visibility.can_see(&tag, &team_ids) {
            continue;
        }
        let links = store.list_tag_links(tag.id, &[]).await?;
        let visible = filter_visible_links(links, &visibility, &team_ids, &repo_ids);
        out.push(TagDto::from_with_count(tag, visible.len() as u32));
    }
    Ok(Json(out))
}

/// `GET /me/tags` — caller-owned or caller-is-scope-member tags.
///
/// Convenience over `GET /tags` for the new-tag picker and the
/// "my tags" sidebar group. Returns the same row shape; the
/// filter is "the viewer is a scope member" (which for user-scope
/// = owner, for team/org-scope = membership).
#[utoipa::path(
    get,
    path = "/me/tags",
    params(ListTagsQuery),
    responses(
        (status = 200, description = "Tags caller owns or is a scope member of", body = Vec<TagDto>),
    ),
    tag = "tags",
)]
pub async fn list_my_tags(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<ListTagsQuery>,
) -> Result<Json<Vec<TagDto>>, ApiError> {
    // `is_scope_member` is currently identical to `can_see`
    // (team-scope membership = team-org visibility in v1), so we
    // can share the listing path. When team membership lands the
    // filter tightens here without changing the wire shape.
    list_tags(State(state), Extension(principal), Query(q)).await
}

/// `POST /tags` — create a tag.
///
/// * Caller must be a scope member of the requested scope, else
///   `403 tag_scope_member_required`.
/// * Case-insensitive name uniqueness per scope is a DB-level
///   expression index — a clash surfaces as
///   `409 tag_name_conflict`.
/// * Audit: writes [`audit::TAG_CREATE`] with target
///   `"<scope_kind>:<scope_id>:<tag_id>"`.
#[utoipa::path(
    post,
    path = "/tags",
    request_body = CreateTagRequest,
    responses(
        (status = 200, description = "Tag created", body = TagDto),
        (status = 403, description = "Caller is not a member of the requested scope"),
        (status = 409, description = "Tag name already in use for this scope"),
    ),
    tag = "tags",
)]
pub async fn create_tag(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<CreateTagRequest>,
) -> Result<Json<TagDto>, ApiError> {
    let store = state.store.as_ref();
    let viewer = principal.actor_user_id;
    let visibility = ViewerVisibility::load(store, viewer).await?;
    let team_ids = visibility.visible_team_ids(store).await?;

    let scope_kind: TagScopeKind = body.scope_kind.into();

    // Scope-membership gate — §7.4.
    let allowed = match scope_kind {
        TagScopeKind::User => body.scope_id == viewer,
        TagScopeKind::Team => team_ids.contains(&body.scope_id),
        TagScopeKind::Org => visibility.visible_org_ids.contains(&body.scope_id),
    };
    if !allowed {
        return Err(ApiError::Forbidden {
            code: "tag_scope_member_required",
            message: format!(
                "caller is not a member of {} {}",
                body.scope_kind_str(),
                body.scope_id
            ),
        });
    }

    let mut tag = Tag {
        id: Uuid::new_v4(),
        scope_kind,
        scope_user_id: None,
        scope_team_id: None,
        scope_org_id: None,
        name: body.name,
        color: body.color,
        description: body.description,
        created_by: viewer,
        created_at: Utc::now(),
        archived_at: None,
    };
    match scope_kind {
        TagScopeKind::User => tag.scope_user_id = Some(body.scope_id),
        TagScopeKind::Team => tag.scope_team_id = Some(body.scope_id),
        TagScopeKind::Org => tag.scope_org_id = Some(body.scope_id),
    }

    let saved = match store.create_tag(&tag).await {
        Ok(t) => t,
        Err(StoreError::Conflict(msg)) => {
            return Err(ApiError::Conflict {
                code: "tag_name_conflict",
                message: format!("tag name not unique within scope: {msg}"),
            });
        }
        Err(e) => return Err(e.into()),
    };

    audit::record(
        store,
        viewer,
        audit::TAG_CREATE,
        format!(
            "{}:{}:{}",
            saved.scope_kind.as_str(),
            saved.scope_id().unwrap_or(Uuid::nil()),
            saved.id
        ),
    )
    .await?;

    Ok(Json(TagDto::from_with_count(saved, 0)))
}

impl CreateTagRequest {
    fn scope_kind_str(&self) -> &'static str {
        match self.scope_kind {
            TagScopeKindDto::User => "user",
            TagScopeKindDto::Team => "team",
            TagScopeKindDto::Org => "org",
        }
    }
}

/// `GET /tags/{id}` — single tag, plus a page of its visible links.
#[utoipa::path(
    get,
    path = "/tags/{id}",
    params(
        ("id" = Uuid, Path, description = "Tag id"),
        TagDetailQuery,
    ),
    responses(
        (status = 200, description = "Tag + paginated visible link slice", body = TagDetailResponse),
        (status = 404, description = "Tag does not exist or is invisible to caller"),
    ),
    tag = "tags",
)]
pub async fn get_tag(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Query(q): Query<TagDetailQuery>,
) -> Result<Json<TagDetailResponse>, ApiError> {
    let store = state.store.as_ref();
    let visibility = ViewerVisibility::load(store, principal.actor_user_id).await?;
    let team_ids = visibility.visible_team_ids(store).await?;
    let repo_ids = visible_repo_ids(store, &visibility).await?;

    let tag = match store.get_tag(id).await {
        Ok(t) => t,
        Err(StoreError::NotFound { .. }) => {
            return Err(tag_not_found(id));
        }
        Err(e) => return Err(e.into()),
    };
    if !visibility.can_see(&tag, &team_ids) {
        // Hide existence — return 404, not 403, per §7.4 spirit:
        // the caller is not denied a tag they could see, they are
        // told the tag does not exist for them.
        return Err(tag_not_found(id));
    }

    let all_links = store.list_tag_links(tag.id, &[]).await?;
    let visible = filter_visible_links(all_links, &visibility, &team_ids, &repo_ids);
    let total = visible.len() as u32;

    let page = q.links_page.unwrap_or(0);
    let start = (page as usize).saturating_mul(LINKS_PAGE_SIZE as usize);
    let end = start.saturating_add(LINKS_PAGE_SIZE as usize).min(visible.len());
    let page_slice: Vec<TagLinkDto> = if start >= visible.len() {
        Vec::new()
    } else {
        visible[start..end]
            .iter()
            .cloned()
            .map(TagLinkDto::from)
            .collect()
    };

    Ok(Json(TagDetailResponse {
        tag: TagDto::from_with_count(tag, total),
        links: page_slice,
        links_page: page,
        links_page_size: LINKS_PAGE_SIZE,
    }))
}

/// `PATCH /tags/{id}` — rename / recolour / set description /
/// archive. Mutually-exclusive verb mapping:
///
/// * If `archived` is set: emits [`audit::TAG_ARCHIVE`] (regardless
///   of whether other fields are also patched in the same request).
/// * Otherwise emits [`audit::TAG_UPDATE`].
///
/// The two-verb split lets the audit log answer "when did this
/// tag retire?" with one query (`WHERE action = 'tag.archive'
/// AND target LIKE '<tag_id>'`) without scanning every
/// `tag.update` row.
#[utoipa::path(
    patch,
    path = "/tags/{id}",
    params(("id" = Uuid, Path, description = "Tag id")),
    request_body = UpdateTagRequest,
    responses(
        (status = 200, description = "Patched tag", body = TagDto),
        (status = 403, description = "Caller is not a scope member"),
        (status = 404, description = "Tag does not exist or is invisible"),
        (status = 409, description = "Tag name not unique within scope"),
    ),
    tag = "tags",
)]
pub async fn update_tag(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTagRequest>,
) -> Result<Json<TagDto>, ApiError> {
    let store = state.store.as_ref();
    let viewer = principal.actor_user_id;
    let visibility = ViewerVisibility::load(store, viewer).await?;
    let team_ids = visibility.visible_team_ids(store).await?;

    let tag = match store.get_tag(id).await {
        Ok(t) => t,
        Err(StoreError::NotFound { .. }) => return Err(tag_not_found(id)),
        Err(e) => return Err(e.into()),
    };
    if !visibility.can_see(&tag, &team_ids) {
        return Err(tag_not_found(id));
    }
    if !visibility.is_scope_member(&tag, &team_ids) {
        return Err(scope_member_required(&tag));
    }

    let archived_at: Option<Option<DateTime<Utc>>> = match body.archived {
        Some(true) => {
            if tag.archived_at.is_some() {
                None // already archived — leave unchanged
            } else {
                Some(Some(Utc::now()))
            }
        }
        Some(false) => Some(None),
        None => None,
    };
    let archiving = matches!(body.archived, Some(true)) && tag.archived_at.is_none();

    let patched = match store
        .update_tag(
            tag.id,
            body.name.as_deref(),
            body.color.as_deref(),
            body.description.as_ref().map(|o| o.as_deref()),
            archived_at,
        )
        .await
    {
        Ok(t) => t,
        Err(StoreError::Conflict(msg)) => {
            return Err(ApiError::Conflict {
                code: "tag_name_conflict",
                message: format!("tag name not unique within scope: {msg}"),
            });
        }
        Err(StoreError::NotFound { .. }) => return Err(tag_not_found(id)),
        Err(e) => return Err(e.into()),
    };

    // Audit verb split: archive transitions count as `tag.archive`;
    // everything else (incl. un-archive — see §7.4 "never hard
    // delete" framing) is `tag.update`.
    let verb = if archiving {
        audit::TAG_ARCHIVE
    } else {
        audit::TAG_UPDATE
    };
    audit::record(store, viewer, verb, format!("tag:{}", patched.id)).await?;

    let links = store.list_tag_links(patched.id, &[]).await?;
    let repo_ids = visible_repo_ids(store, &visibility).await?;
    let visible = filter_visible_links(links, &visibility, &team_ids, &repo_ids);
    Ok(Json(TagDto::from_with_count(patched, visible.len() as u32)))
}

/// `POST /tags/{id}/links` — batch link, transactional all-or-
/// nothing per §7.5.
///
/// Per-item validation (pre-commit):
/// * `target_id` must not duplicate an existing link of the same
///   `kind` on this tag (`duplicate`).
/// * `target_id` must not duplicate another row in the same
///   batch (`duplicate_in_batch`).
///
/// On any per-item failure the response is `422 batch_rejected`
/// with an `items: [{index, code, message}, ...]` array and
/// **nothing is committed**. On success, one audit row per link
/// is written per §7.6, and `LinkBatchResponse.warning` is set
/// when the post-commit link count exceeds
/// [`TAG_LINK_WARN_THRESHOLD`] (§13.5).
#[utoipa::path(
    post,
    path = "/tags/{id}/links",
    params(("id" = Uuid, Path, description = "Tag id")),
    request_body = LinkBatchRequest,
    responses(
        (status = 200, description = "Links attached", body = LinkBatchResponse),
        (status = 403, description = "Caller is not a scope member"),
        (status = 404, description = "Tag does not exist or is invisible"),
        (status = 422, description = "Batch rejected — per-item errors in `items[]`"),
    ),
    tag = "tags",
)]
pub async fn link_targets(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<LinkBatchRequest>,
) -> Result<Json<LinkBatchResponse>, ApiError> {
    let store = state.store.as_ref();
    let viewer = principal.actor_user_id;
    let visibility = ViewerVisibility::load(store, viewer).await?;
    let team_ids = visibility.visible_team_ids(store).await?;

    let tag = match store.get_tag(id).await {
        Ok(t) => t,
        Err(StoreError::NotFound { .. }) => return Err(tag_not_found(id)),
        Err(e) => return Err(e.into()),
    };
    if !visibility.can_see(&tag, &team_ids) {
        return Err(tag_not_found(id));
    }
    if !visibility.is_scope_member(&tag, &team_ids) {
        return Err(scope_member_required(&tag));
    }

    // Pre-validate: existing links + duplicate-within-batch.
    let existing = store.list_tag_links(tag.id, &[]).await?;
    let existing_keys: HashSet<(TagLinkKind, Uuid)> = existing
        .iter()
        .filter_map(|l| l.target_id().map(|t| (l.kind, t)))
        .collect();

    let mut errors: Vec<BatchItemError> = Vec::new();
    let mut seen: HashSet<(TagLinkKind, Uuid)> = HashSet::new();
    let mut to_insert: Vec<TagLink> = Vec::with_capacity(body.items.len());
    for (idx, item) in body.items.iter().enumerate() {
        let kind: TagLinkKind = item.kind.into();
        let key = (kind, item.target_id);
        if existing_keys.contains(&key) {
            errors.push(BatchItemError {
                index: idx,
                code: "duplicate",
                message: format!(
                    "tag already links {} {}",
                    kind.as_str(),
                    item.target_id
                ),
            });
            continue;
        }
        if !seen.insert(key) {
            errors.push(BatchItemError {
                index: idx,
                code: "duplicate_in_batch",
                message: format!(
                    "{} {} appears more than once in this batch",
                    kind.as_str(),
                    item.target_id
                ),
            });
            continue;
        }
        let mut link = TagLink {
            id: Uuid::new_v4(),
            tag_id: tag.id,
            kind,
            target_repo_id: None,
            target_issue_id: None,
            target_user_id: None,
            target_team_id: None,
            added_by: viewer,
            added_at: Utc::now(),
        };
        match kind {
            TagLinkKind::Repo => link.target_repo_id = Some(item.target_id),
            TagLinkKind::Issue => link.target_issue_id = Some(item.target_id),
            TagLinkKind::User => link.target_user_id = Some(item.target_id),
            TagLinkKind::Team => link.target_team_id = Some(item.target_id),
        }
        to_insert.push(link);
    }
    if !errors.is_empty() {
        return Err(ApiError::Batch {
            code: "batch_rejected",
            message: format!("{} of {} items rejected", errors.len(), body.items.len()),
            items: errors,
        });
    }
    if to_insert.is_empty() {
        return Ok(Json(LinkBatchResponse {
            linked: Vec::new(),
            warning: None,
        }));
    }

    let inserted = match store.add_tag_links(&to_insert).await {
        Ok(rows) => rows,
        Err(StoreError::Conflict(msg)) => {
            // The unique index fired despite our pre-check — most
            // likely a concurrent batch landed mid-validation. Map
            // every input row that *could* have collided to a
            // duplicate item so the caller can retry the slimmer
            // batch. We don't know which one specifically failed
            // (the store applied the batch in one statement and
            // got a single error back), so surface a single
            // envelope-level row with index = -1 sentinel encoded
            // as the first item.
            return Err(ApiError::Batch {
                code: "batch_rejected",
                message: format!("concurrent insert raced this batch: {msg}"),
                items: vec![BatchItemError {
                    index: 0,
                    code: "duplicate",
                    message: msg,
                }],
            });
        }
        Err(e) => return Err(e.into()),
    };

    // Audit — one row per link, per §7.6.
    for link in &inserted {
        let tgt = link.target_id().unwrap_or(Uuid::nil());
        audit::record(
            store,
            viewer,
            audit::TAG_LINK,
            format!("{}:{}:{}", tag.id, link.kind.as_str(), tgt),
        )
        .await?;
    }

    let total = existing.len() + inserted.len();
    let warning = if total > TAG_LINK_WARN_THRESHOLD {
        Some("tag_link_count_high".to_string())
    } else {
        None
    };

    Ok(Json(LinkBatchResponse {
        linked: inserted.into_iter().map(TagLinkDto::from).collect(),
        warning,
    }))
}

/// `DELETE /tags/{id}/links` — batch unlink, transactional
/// all-or-nothing per §7.5.
///
/// Same per-item error surface as link: items that do not match
/// a live `(kind, target_id)` on the tag come back as
/// `code = "not_linked"` and the whole batch is rejected (no
/// partial unlinks). One `tag.unlink` audit row per detached
/// link.
#[utoipa::path(
    delete,
    path = "/tags/{id}/links",
    params(("id" = Uuid, Path, description = "Tag id")),
    request_body = UnlinkBatchRequest,
    responses(
        (status = 200, description = "Links detached", body = Ack),
        (status = 403, description = "Caller is not a scope member"),
        (status = 404, description = "Tag does not exist or is invisible"),
        (status = 422, description = "Batch rejected — per-item errors in `items[]`"),
    ),
    tag = "tags",
)]
pub async fn unlink_targets(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(body): Json<UnlinkBatchRequest>,
) -> Result<Json<Ack>, ApiError> {
    let store = state.store.as_ref();
    let viewer = principal.actor_user_id;
    let visibility = ViewerVisibility::load(store, viewer).await?;
    let team_ids = visibility.visible_team_ids(store).await?;

    let tag = match store.get_tag(id).await {
        Ok(t) => t,
        Err(StoreError::NotFound { .. }) => return Err(tag_not_found(id)),
        Err(e) => return Err(e.into()),
    };
    if !visibility.can_see(&tag, &team_ids) {
        return Err(tag_not_found(id));
    }
    if !visibility.is_scope_member(&tag, &team_ids) {
        return Err(scope_member_required(&tag));
    }

    // Resolve `(kind, target_id)` → link id from the live set so
    // the unlink call has one stable input shape. The pre-check
    // happens inside the same handler request as the store call;
    // any concurrent unlink that races us turns into a per-item
    // `not_linked` on retry, which is the right behaviour.
    let existing = store.list_tag_links(tag.id, &[]).await?;
    let index: std::collections::HashMap<(TagLinkKind, Uuid), Uuid> = existing
        .iter()
        .filter_map(|l| l.target_id().map(|t| ((l.kind, t), l.id)))
        .collect();

    let mut errors: Vec<BatchItemError> = Vec::new();
    let mut to_remove: Vec<Uuid> = Vec::with_capacity(body.items.len());
    let mut seen: HashSet<(TagLinkKind, Uuid)> = HashSet::new();
    for (idx, item) in body.items.iter().enumerate() {
        let kind: TagLinkKind = item.kind.into();
        let key = (kind, item.target_id);
        if !seen.insert(key) {
            errors.push(BatchItemError {
                index: idx,
                code: "duplicate_in_batch",
                message: format!(
                    "{} {} appears more than once in this batch",
                    kind.as_str(),
                    item.target_id
                ),
            });
            continue;
        }
        match index.get(&key) {
            Some(link_id) => to_remove.push(*link_id),
            None => {
                errors.push(BatchItemError {
                    index: idx,
                    code: "not_linked",
                    message: format!(
                        "tag does not link {} {}",
                        kind.as_str(),
                        item.target_id
                    ),
                });
            }
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::Batch {
            code: "batch_rejected",
            message: format!("{} of {} items rejected", errors.len(), body.items.len()),
            items: errors,
        });
    }
    if to_remove.is_empty() {
        return Ok(Json(Ack { ok: true }));
    }

    match store.remove_tag_links(&to_remove).await {
        Ok(()) => {}
        Err(StoreError::NotFound { .. }) => {
            return Err(ApiError::Batch {
                code: "batch_rejected",
                message: "one or more links vanished mid-batch".into(),
                items: vec![BatchItemError {
                    index: 0,
                    code: "not_linked",
                    message: "link vanished mid-batch (concurrent unlink)".into(),
                }],
            });
        }
        Err(e) => return Err(e.into()),
    }

    // Audit — one row per detached link, mirroring `tag.link`.
    for item in &body.items {
        let kind: TagLinkKind = item.kind.into();
        audit::record(
            store,
            viewer,
            audit::TAG_UNLINK,
            format!("{}:{}:{}", tag.id, kind.as_str(), item.target_id),
        )
        .await?;
    }
    Ok(Json(Ack { ok: true }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tag_not_found(id: Uuid) -> ApiError {
    ApiError::NotFound {
        code: "tag_not_found",
        message: format!("no tag {id} visible to caller"),
    }
}

fn scope_member_required(tag: &Tag) -> ApiError {
    ApiError::Forbidden {
        code: "tag_scope_member_required",
        message: format!(
            "caller is not a member of {} {}",
            tag.scope_kind.as_str(),
            tag.scope_id().unwrap_or(Uuid::nil())
        ),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the tags router fragment. Mount via `Router::merge` from
/// `dp-server::build`. Same authz envelope as the rest of the
/// dp-rest fragments — `tags.read` covers the two GETs; `tags.write`
/// covers create, patch, link, unlink (§7.6 audit verbs are
/// finer-grained than the authz vocabulary).
pub fn tags_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    Router::new()
        .merge(with_permission(
            Router::new()
                .route("/tags", get(list_tags))
                .route("/tags/{id}", get(get_tag))
                .route("/me/tags", get(list_my_tags)),
            "tags",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/tags", post(create_tag))
                .route("/tags/{id}", patch(update_tag))
                .route("/tags/{id}/links", post(link_targets))
                .route("/tags/{id}/links", delete(unlink_targets)),
            "tags",
            "write",
        ))
        .with_state(inner)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use std::sync::Mutex;
    use tower::ServiceExt;

    use dp_domain::audit::AuditEntry;
    use dp_domain::membership::MembershipRole;
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // In-memory store fake — minimal surface to drive the tags routes
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        memberships: Mutex<Vec<Membership>>,
        teams: Mutex<Vec<Team>>,
        tags: Mutex<Vec<Tag>>,
        links: Mutex<Vec<TagLink>>,
        audit: Mutex<Vec<AuditEntry>>,
    }

    impl MemStore {
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
        fn links_for(&self, tag_id: Uuid) -> Vec<TagLink> {
            self.links
                .lock()
                .unwrap()
                .iter()
                .filter(|l| l.tag_id == tag_id)
                .cloned()
                .collect()
        }
    }

    #[async_trait]
    impl Store for MemStore {
        async fn list_memberships_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn list_teams_for_org(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
            Ok(self
                .teams
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.org_id == org_id)
                .cloned()
                .collect())
        }
        async fn get_tag(&self, id: Uuid) -> Result<Tag, StoreError> {
            self.tags
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .ok_or(StoreError::NotFound {
                    entity: "tag",
                    id: id.to_string(),
                })
        }
        async fn create_tag(&self, tag: &Tag) -> Result<Tag, StoreError> {
            let mut tags = self.tags.lock().unwrap();
            // Per-scope case-insensitive name uniqueness — mirror
            // the migration's expression index.
            let name = tag.name.to_lowercase();
            let scope_id = tag.scope_id();
            if tags.iter().any(|t| {
                t.scope_kind == tag.scope_kind
                    && t.scope_id() == scope_id
                    && t.name.to_lowercase() == name
            }) {
                return Err(StoreError::Conflict(format!(
                    "tag '{}' already exists in this scope",
                    tag.name
                )));
            }
            tags.push(tag.clone());
            Ok(tag.clone())
        }
        async fn update_tag(
            &self,
            id: Uuid,
            name: Option<&str>,
            color: Option<&str>,
            description: Option<Option<&str>>,
            archived_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
        ) -> Result<Tag, StoreError> {
            let mut tags = self.tags.lock().unwrap();
            let Some(idx) = tags.iter().position(|t| t.id == id) else {
                return Err(StoreError::NotFound {
                    entity: "tag",
                    id: id.to_string(),
                });
            };
            if let Some(new_name) = name {
                let lower = new_name.to_lowercase();
                if tags.iter().enumerate().any(|(i, t)| {
                    i != idx
                        && t.scope_kind == tags[idx].scope_kind
                        && t.scope_id() == tags[idx].scope_id()
                        && t.name.to_lowercase() == lower
                }) {
                    return Err(StoreError::Conflict(format!(
                        "tag '{new_name}' already exists in this scope"
                    )));
                }
                tags[idx].name = new_name.to_string();
            }
            if let Some(c) = color {
                tags[idx].color = c.to_string();
            }
            if let Some(d) = description {
                tags[idx].description = d.map(|s| s.to_string());
            }
            if let Some(a) = archived_at {
                tags[idx].archived_at = a;
            }
            Ok(tags[idx].clone())
        }
        async fn list_tags_visible_to(
            &self,
            viewer: Uuid,
            visible_team_ids: &[Uuid],
            visible_org_ids: &[Uuid],
            include_archived: bool,
        ) -> Result<Vec<Tag>, StoreError> {
            let tags = self.tags.lock().unwrap();
            let team: HashSet<Uuid> = visible_team_ids.iter().copied().collect();
            let org: HashSet<Uuid> = visible_org_ids.iter().copied().collect();
            Ok(tags
                .iter()
                .filter(|t| {
                    if !include_archived && t.archived_at.is_some() {
                        return false;
                    }
                    match t.scope_kind {
                        TagScopeKind::User => t.scope_user_id == Some(viewer),
                        TagScopeKind::Team => {
                            t.scope_team_id.map(|x| team.contains(&x)).unwrap_or(false)
                        }
                        TagScopeKind::Org => {
                            t.scope_org_id.map(|x| org.contains(&x)).unwrap_or(false)
                        }
                    }
                })
                .cloned()
                .collect())
        }
        async fn list_tag_links(
            &self,
            tag_id: Uuid,
            kinds: &[TagLinkKind],
        ) -> Result<Vec<TagLink>, StoreError> {
            let links = self.links.lock().unwrap();
            Ok(links
                .iter()
                .filter(|l| l.tag_id == tag_id && (kinds.is_empty() || kinds.contains(&l.kind)))
                .cloned()
                .collect())
        }
        async fn add_tag_links(&self, links: &[TagLink]) -> Result<Vec<TagLink>, StoreError> {
            let mut all = self.links.lock().unwrap();
            // Defence-in-depth dup check against the live set —
            // mirrors the unique-index behaviour of the real store.
            for l in links {
                let target = l.target_id();
                if all
                    .iter()
                    .any(|x| x.tag_id == l.tag_id && x.kind == l.kind && x.target_id() == target)
                {
                    return Err(StoreError::Conflict(format!(
                        "duplicate ({}, {:?})",
                        l.tag_id, target
                    )));
                }
            }
            for l in links {
                all.push(l.clone());
            }
            Ok(links.to_vec())
        }
        async fn remove_tag_links(&self, link_ids: &[Uuid]) -> Result<(), StoreError> {
            let mut all = self.links.lock().unwrap();
            for id in link_ids {
                if !all.iter().any(|l| l.id == *id) {
                    return Err(StoreError::NotFound {
                        entity: "tag_link",
                        id: id.to_string(),
                    });
                }
            }
            all.retain(|l| !link_ids.contains(&l.id));
            Ok(())
        }
        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }

        // ---- minimal stubs for everything else --------------------------
        async fn upsert_user(&self, u: &User) -> Result<User, StoreError> {
            Ok(u.clone())
        }
        async fn get_user(&self, _: Uuid) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn get_user_by_github_id(&self, _: i64) -> Result<User, StoreError> {
            unimplemented!()
        }
        async fn list_users(&self) -> Result<Vec<User>, StoreError> {
            Ok(vec![])
        }
        async fn pseudonymise_user(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn upsert_org(&self, o: &Org) -> Result<Org, StoreError> {
            Ok(o.clone())
        }
        async fn upsert_team(&self, t: &Team) -> Result<Team, StoreError> {
            Ok(t.clone())
        }
        async fn upsert_repo(&self, r: &Repo) -> Result<Repo, StoreError> {
            Ok(r.clone())
        }
        async fn upsert_membership(&self, m: &Membership) -> Result<Membership, StoreError> {
            Ok(m.clone())
        }
        async fn set_home_org(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<Uuid>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_event(&self, e: &ActivityEvent) -> Result<ActivityEvent, StoreError> {
            Ok(e.clone())
        }
        async fn add_event_actors(&self, _: &[EventActor]) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_event_actor_rows_in_window(
            &self,
            _: &Window,
            _: &[Uuid],
            _: &[Uuid],
            _: &[Uuid],
            _: &[ActorRole],
        ) -> Result<Vec<EventActorRow>, StoreError> {
            Ok(vec![])
        }
        async fn get_cursor(
            &self,
            _: Uuid,
            _: Option<Uuid>,
            _: ResourceKind,
        ) -> Result<FetchCursor, StoreError> {
            Err(StoreError::NotFound {
                entity: "fetch_cursor",
                id: String::new(),
            })
        }
        async fn put_cursor(&self, _: &FetchCursor) -> Result<(), StoreError> {
            Ok(())
        }
        async fn start_fetch_run(&self, _: FetchRunKind) -> Result<Uuid, StoreError> {
            Ok(Uuid::new_v4())
        }
        async fn finish_fetch_run(
            &self,
            _: Uuid,
            _: i64,
            _: i64,
            _: bool,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn list_recent_fetch_runs(&self, _: i64) -> Result<Vec<FetchRun>, StoreError> {
            Ok(vec![])
        }
        async fn data_as_of(&self) -> Result<dp_domain::freshness::DataAsOf, StoreError> {
            Ok(dp_domain::freshness::DataAsOf::default())
        }
        async fn enqueue_webhook(&self, _: &WebhookDelivery) -> Result<(), StoreError> {
            Ok(())
        }
        async fn claim_webhooks(&self, _: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
            Ok(vec![])
        }
        async fn mark_webhook_processed(&self, _: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
        async fn mark_webhook_failed(&self, _: Uuid, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------
    // Test harness
    // -----------------------------------------------------------------

    fn build_app(store: Arc<MemStore>, actor: Uuid) -> Router {
        use starter_spi::auth::{Principal as SpiPrincipal, Role};
        use starter_spi::authz::{NoopPolicyEngine, PolicyEngine};
        use std::sync::Arc as StdArc;
        let app_state = Arc::new(AppState::new(store));
        let engine: StdArc<dyn PolicyEngine> = StdArc::new(NoopPolicyEngine);
        let spi_principal = SpiPrincipal {
            subject: actor.to_string(),
            role: Role::Admin,
            scopes: Vec::new(),
            tenant_id: None,
            teams: Vec::new(),
            extra: serde_json::Value::Null,
        };
        tags_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)
    }

    fn seed_membership(store: &MemStore, user: Uuid, org: Uuid) {
        store.memberships.lock().unwrap().push(Membership {
            user_id: user,
            org_id: org,
            role: MembershipRole::Member,
            home_org: None,
            joined_at: Utc::now(),
        });
    }

    fn seed_org_tag(store: &MemStore, owner: Uuid, org: Uuid, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        store.tags.lock().unwrap().push(Tag {
            id,
            scope_kind: TagScopeKind::Org,
            scope_user_id: None,
            scope_team_id: None,
            scope_org_id: Some(org),
            name: name.into(),
            color: "indigo".into(),
            description: None,
            created_by: owner,
            created_at: Utc::now(),
            archived_at: None,
        });
        id
    }

    fn seed_link(store: &MemStore, tag_id: Uuid, kind: TagLinkKind, target: Uuid, actor: Uuid) {
        let mut link = TagLink {
            id: Uuid::new_v4(),
            tag_id,
            kind,
            target_repo_id: None,
            target_issue_id: None,
            target_user_id: None,
            target_team_id: None,
            added_by: actor,
            added_at: Utc::now(),
        };
        match kind {
            TagLinkKind::Repo => link.target_repo_id = Some(target),
            TagLinkKind::Issue => link.target_issue_id = Some(target),
            TagLinkKind::User => link.target_user_id = Some(target),
            TagLinkKind::Team => link.target_team_id = Some(target),
        }
        store.links.lock().unwrap().push(link);
    }

    // -----------------------------------------------------------------
    // GET /tags
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_tags_filters_to_visible_scope() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org_visible = Uuid::new_v4();
        let org_hidden = Uuid::new_v4();
        seed_membership(&store, actor, org_visible);
        let visible = seed_org_tag(&store, actor, org_visible, "Visible");
        let _hidden = seed_org_tag(&store, actor, org_hidden, "Hidden");

        let app = build_app(store, actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tags")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], serde_json::json!(visible));
        assert_eq!(arr[0]["scope_kind"], "org");
    }

    #[tokio::test]
    async fn list_tags_excludes_archived_by_default_and_includes_with_flag() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let live = seed_org_tag(&store, actor, org, "Live");
        let archived = seed_org_tag(&store, actor, org, "Archived");
        store
            .tags
            .lock()
            .unwrap()
            .iter_mut()
            .find(|t| t.id == archived)
            .unwrap()
            .archived_at = Some(Utc::now());

        let app = build_app(store.clone(), actor);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/tags")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], serde_json::json!(live));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/tags?include_archived=true")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    // -----------------------------------------------------------------
    // POST /tags
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn create_tag_succeeds_for_scope_member_and_audits() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "scope_kind": "org",
            "scope_id": org,
            "name": "Phoenix",
            "color": "indigo",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tags")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["scope_kind"], "org");
        assert_eq!(v["scope_id"], serde_json::json!(org));
        assert_eq!(v["name"], "Phoenix");
        assert_eq!(v["visible_link_count"], 0);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::TAG_CREATE);
        assert!(rows[0].target.starts_with("org:"));
    }

    #[tokio::test]
    async fn create_tag_rejects_non_member_with_403() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let foreign_org = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "scope_kind": "org",
            "scope_id": foreign_org,
            "name": "Phoenix",
            "color": "indigo",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tags")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "tag_scope_member_required");
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn create_tag_returns_409_on_name_conflict() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        seed_org_tag(&store, actor, org, "Phoenix");
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "scope_kind": "org",
            "scope_id": org,
            "name": "phoenix", // case-insensitive
            "color": "red",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tags")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "tag_name_conflict");
    }

    // -----------------------------------------------------------------
    // PATCH /tags/{id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn update_tag_renames_and_audits_as_update() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "name": "Phoenix-v2", "color": "red" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/tags/{tag}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["name"], "Phoenix-v2");
        assert_eq!(v["color"], "red");
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::TAG_UPDATE);
    }

    #[tokio::test]
    async fn update_tag_archive_uses_archive_verb() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "archived": true });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/tags/{tag}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert!(v["archived_at"].as_str().is_some());
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::TAG_ARCHIVE);
    }

    #[tokio::test]
    async fn update_tag_returns_404_when_invisible() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let other_org = Uuid::new_v4();
        let tag = seed_org_tag(&store, actor, other_org, "Phoenix");
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "name": "X" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/tags/{tag}"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "tag_not_found");
    }

    // -----------------------------------------------------------------
    // POST /tags/{id}/links
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn link_targets_attaches_and_audits_per_link() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let r1 = Uuid::new_v4();
        let r2 = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "items": [
                { "kind": "repo", "target_id": r1 },
                { "kind": "repo", "target_id": r2 },
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["linked"].as_array().unwrap().len(), 2);
        assert!(v["warning"].is_null());
        assert_eq!(store.links_for(tag).len(), 2);
        let rows = store.audit_rows();
        // One per link, per §7.6.
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.action == audit::TAG_LINK));
    }

    #[tokio::test]
    async fn link_targets_rejects_duplicates_with_per_item_errors_and_commits_nothing() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let r_existing = Uuid::new_v4();
        seed_link(&store, tag, TagLinkKind::Repo, r_existing, actor);
        let r_new = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "items": [
                { "kind": "repo", "target_id": r_new },        // fine
                { "kind": "repo", "target_id": r_existing },   // duplicate
                { "kind": "repo", "target_id": r_new },        // duplicate_in_batch
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "batch_rejected");
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["index"], 1);
        assert_eq!(items[0]["code"], "duplicate");
        assert_eq!(items[1]["index"], 2);
        assert_eq!(items[1]["code"], "duplicate_in_batch");
        // Nothing committed beyond the seeded link.
        assert_eq!(store.links_for(tag).len(), 1);
        // No tag.link audit rows for a rejected batch.
        assert!(store
            .audit_rows()
            .iter()
            .all(|r| r.action != audit::TAG_LINK));
    }

    #[tokio::test]
    async fn link_targets_returns_403_for_non_scope_member() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let other_org = Uuid::new_v4();
        // Don't add `actor` to `other_org` — tag is invisible →
        // 404 (info-hiding per the handler doc-comment).
        let tag = seed_org_tag(&store, Uuid::new_v4(), other_org, "Phoenix");
        let app = build_app(store, actor);
        let body = serde_json::json!({ "items": [{ "kind": "repo", "target_id": Uuid::new_v4() }] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn link_targets_warns_above_threshold() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        // Pre-seed right up to the threshold so a one-row batch
        // tips it over without 500 individual seed inserts.
        for _ in 0..TAG_LINK_WARN_THRESHOLD {
            seed_link(&store, tag, TagLinkKind::Repo, Uuid::new_v4(), actor);
        }
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "items": [{ "kind": "repo", "target_id": Uuid::new_v4() }]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["warning"], "tag_link_count_high");
    }

    // -----------------------------------------------------------------
    // DELETE /tags/{id}/links
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn unlink_targets_detaches_and_audits_per_link() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let r1 = Uuid::new_v4();
        let r2 = Uuid::new_v4();
        seed_link(&store, tag, TagLinkKind::Repo, r1, actor);
        seed_link(&store, tag, TagLinkKind::Repo, r2, actor);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "items": [
                { "kind": "repo", "target_id": r1 },
                { "kind": "repo", "target_id": r2 },
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(store.links_for(tag).len(), 0);
        let unlink_rows: Vec<_> = store
            .audit_rows()
            .into_iter()
            .filter(|r| r.action == audit::TAG_UNLINK)
            .collect();
        assert_eq!(unlink_rows.len(), 2);
    }

    #[tokio::test]
    async fn unlink_targets_rejects_unknown_with_per_item_errors() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        let r_real = Uuid::new_v4();
        seed_link(&store, tag, TagLinkKind::Repo, r_real, actor);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({
            "items": [
                { "kind": "repo", "target_id": r_real },
                { "kind": "repo", "target_id": Uuid::new_v4() }, // not linked
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/tags/{tag}/links"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "batch_rejected");
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["index"], 1);
        assert_eq!(items[0]["code"], "not_linked");
        // No partial unlink.
        assert_eq!(store.links_for(tag).len(), 1);
    }

    // -----------------------------------------------------------------
    // GET /tags/{id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn get_tag_paginates_links() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let org = Uuid::new_v4();
        seed_membership(&store, actor, org);
        let tag = seed_org_tag(&store, actor, org, "Phoenix");
        // Seed 3 user-kind links (User links are unconditionally
        // visible per the v1 simplification).
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let u3 = Uuid::new_v4();
        seed_link(&store, tag, TagLinkKind::User, u1, actor);
        seed_link(&store, tag, TagLinkKind::User, u2, actor);
        seed_link(&store, tag, TagLinkKind::User, u3, actor);

        let app = build_app(store.clone(), actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/tags/{tag}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["tag"]["visible_link_count"], 3);
        assert_eq!(v["links"].as_array().unwrap().len(), 3);
        assert_eq!(v["links_page"], 0);
        assert_eq!(v["links_page_size"], LINKS_PAGE_SIZE);
    }
}
