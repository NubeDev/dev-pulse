//! [`ProjectView`] — saved (group_by, filter, sort) bundles per
//! `(project, user)` (`PROJECT-VIEW.md` §5.4 / §6.1, Slice 4).
//!
//! A saved view is a named, persisted pin of the workbench toolbar
//! state. The §5.4 URL precedence is enforced client-side — this
//! module only stores and serves the bundle; dirty-state diffing
//! and the `[Save changes]` affordance live in the frontend.
//!
//! `filter_clauses` is the canonical, parsed shape; the REST layer
//! validates the inbound JSON against this enum before handing it to
//! the store, so the JSONB column never carries an unknown dim.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row in `dp_project_views`. Read shape for the
/// `<ViewsTabStrip>` and the `?view=<id>` precedence resolver
/// (`PROJECT-VIEW.md` §5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectView {
    /// Primary key.
    pub id: Uuid,
    /// Parent project. Views never move between projects.
    pub project_id: Uuid,
    /// Owner. v1 ships private only — every view is owned by
    /// exactly one user and only that user can read or mutate it.
    pub owner_user_id: Uuid,
    /// Tab label. 1..=60 chars per the migration CHECK.
    pub name: String,
    /// Group-by dimension, or `None` for a flat view. Mirrors the
    /// workbench dropdown values: `Some("status")` or
    /// `Some("tag:<key>")`.
    pub group_by: Option<String>,
    /// Canonical filter clauses (`PROJECT-VIEW.md` §6.1). Stored
    /// in `filter_json` and serialised to the wire `;`-form by
    /// the workbench (§5.4).
    pub filter_clauses: Vec<ProjectViewFilterClause>,
    /// Sort order. Wire values match
    /// `dp_rest::project_issues::parse_sort` —
    /// `updated_desc` (default), `updated_asc`, `title_asc`.
    pub sort: String,
    /// Per-owner ordering within the project. Lower values render
    /// first in the tab strip.
    pub position: i32,
    /// Visibility. v1 always `Private`; `Project` is reserved.
    pub visibility: ProjectViewVisibility,
    /// Optional start date for the view's timeline. Independent of
    /// the parent project's `start_at`. Stored as a tz-agnostic
    /// DATE; rendered in AU `dd/mm/yyyy` by the workbench.
    pub start_date: Option<NaiveDate>,
    /// Optional due date for the view's timeline. Same shape and
    /// rendering as [`Self::start_date`].
    pub due_date: Option<NaiveDate>,
    /// Ordered list of category slugs (lowercase, `[a-z0-9_-]{1,50}`)
    /// rendered as collapsible sections inside the view. Empty
    /// vector — flat view (whatever `group_by` says); non-empty —
    /// the workbench forces `group_by = "tag:category"` and renders
    /// one section per slug in this order, including empty sections.
    /// Issues whose `tag:category` value isn't in this list fall into
    /// a trailing "Uncategorised" section.
    pub categories: Vec<String>,
    /// First write.
    pub created_at: DateTime<Utc>,
    /// Most recent mutation to any field.
    pub updated_at: DateTime<Utc>,
}

/// Visibility enum mirroring `dp_project_views.visibility`. v1
/// only writes `Private`; `Project` is reserved so the shared-views
/// slice doesn't need another migration (PROJECT-VIEW.md §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectViewVisibility {
    /// Owner-only (v1 default).
    Private,
    /// Visible to every project member. Reserved.
    Project,
}

impl ProjectViewVisibility {
    /// Wire form for SQL / JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProjectViewVisibility::Private => "private",
            ProjectViewVisibility::Project => "project",
        }
    }

    /// Parse. Unknown values map to `None` so callers can surface a
    /// `StoreError::Invalid` with the offending value attached.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "private" => Some(Self::Private),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// One canonical filter clause inside a saved view's
/// `filter_clauses` array. The wire JSON shape is the same enum
/// `#[serde(tag = "dim")]` discriminated; the REST validator in
/// `dp-rest::project_views` is the only writer.
///
/// Each variant carries exactly the fields named in
/// PROJECT-VIEW.md §6.1's `filter_json` block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "dim", rename_all = "lowercase")]
pub enum ProjectViewFilterClause {
    /// `{"dim":"status","value":"open"|"closed"}` —
    /// constrained by validator.
    Status {
        /// `"open"` or `"closed"`.
        value: String,
    },
    /// `{"dim":"assignee","value":"<github-login>"}`.
    Assignee {
        /// GitHub login.
        value: String,
    },
    /// `{"dim":"label","value":"<label-name>"}`.
    Label {
        /// Label name (case-insensitive match).
        value: String,
    },
    /// `{"dim":"tag","key":"<tag-key>","value":"<tag-value>"}`.
    Tag {
        /// Tag key.
        key: String,
        /// Tag value.
        value: String,
    },
    /// `{"dim":"milestone","value":"<milestone-uuid>"}` — the
    /// `dp_milestones.id` UUID of an adopted milestone. The wire
    /// value is a stringified UUID; the REST validator parses /
    /// re-stringifies it so the stored JSON is always canonical.
    Milestone {
        /// Stringified `dp_milestones.id` UUID.
        value: String,
    },
}

/// Mutable payload for [`crate::store::Store::create_project_view`]
/// and `update_project_view`. The store fills in `id`,
/// `created_at`, `updated_at`, and (on create) `position`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectViewUpsert {
    /// Tab label.
    pub name: String,
    /// Group-by dimension; see [`ProjectView::group_by`].
    pub group_by: Option<String>,
    /// Canonical filter clauses (validated upstream).
    pub filter_clauses: Vec<ProjectViewFilterClause>,
    /// Sort order. Defaults to `"updated_desc"` when empty.
    pub sort: String,
    /// Visibility. v1 callers always pass `Private`.
    pub visibility: ProjectViewVisibility,
    /// Optional start date for the view's timeline.
    pub start_date: Option<NaiveDate>,
    /// Optional due date for the view's timeline.
    pub due_date: Option<NaiveDate>,
    /// Ordered category slugs. See [`ProjectView::categories`].
    /// Validated upstream (lowercase, `[a-z0-9_-]{1,50}`, deduped,
    /// max 32 items — the workbench can't usefully render more).
    pub categories: Vec<String>,
}
