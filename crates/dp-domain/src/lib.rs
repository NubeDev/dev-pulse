//! `dp-domain` — entity types and the `Store` trait for dev-pulse.
//!
//! This crate is **storage-agnostic**. It defines the value types that
//! flow through every dev-pulse surface (HTTP, MCP, CLI, reports,
//! fetcher) and the [`Store`] trait that the postgres implementation
//! (`dp-store-pg`) satisfies.
//!
//! Hard boundary rule (TODO §0.6): **zero `starter_*` imports** in this
//! crate. `scripts/check-boundaries.sh` enforces this in CI from
//! stage 2 onward.
//!
//! Entities mirror the schema decisions in TODO §0.2–§0.5:
//!
//! * Events are split from their actors — [`ActivityEvent`] carries no
//!   `user_id`; attribution lives in [`EventActor`] rows keyed by
//!   `(event_id, user_id, role)`.
//! * Resumable cursors are per-`(org_id, repo_id, resource_kind)` —
//!   see [`FetchCursor`]. [`FetchRun`] is a run log only.
//! * Soft-delete + pseudonymisation lives on [`User::deleted_at`].
//! * All timestamps are `chrono::DateTime<Utc>` (no naive times).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod event;
pub mod fetch;
pub mod membership;
pub mod org;
pub mod repo;
pub mod store;
pub mod team;
pub mod user;
pub mod webhook;
pub mod window;

pub use event::{ActivityEvent, ActorRole, EventActor, EventKind};
pub use fetch::{FetchCursor, FetchRun, FetchRunKind, ResourceKind};
pub use membership::{Membership, MembershipRole};
pub use org::Org;
pub use repo::Repo;
pub use store::{Store, StoreError};
pub use team::Team;
pub use user::User;
pub use webhook::WebhookDelivery;
pub use window::{Window, WindowAnchor};
