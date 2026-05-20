//! Pins handlers — the per-user workflow surface (SCOPE-PROJECTS §6).
//!
//! The four routes in §6.4 land here:
//!
//! | route                                  | what it does                                              |
//! |----------------------------------------|-----------------------------------------------------------|
//! | `GET    /me/pins`                      | ordered list of the caller's pins                          |
//! | `POST   /me/pins`                      | append a pin (`{kind, target_id}`)                         |
//! | `DELETE /me/pins/{kind}/{id}`          | remove one pin                                             |
//! | `PUT    /me/pins/order`                | atomic rewrite of the caller's pin order                   |
//!
//! Behaviour highlights pinned by SCOPE-PROJECTS §6:
//!
//! * Pins are **strictly per-caller** — every handler keys off
//!   [`Principal::actor_user_id`], never a `?user_id=` query knob. The
//!   `/me/...` prefix is the only addressing scheme. There is no
//!   admin-on-behalf path in v1.
//! * The `POST` path enforces the §13.5 **pin cap** (working
//!   assumption 20) — the REST layer pre-checks against the live row
//!   count and the store layer enforces again as defence-in-depth.
//!   Over-cap inserts return `400 pin_cap_exceeded`, never silently
//!   drop (§6.1).
//! * Newly-added pins go to the **end** (`position = len(existing)`),
//!   matching §6.1 "Newly-added pins go to the end."
//! * `PUT /me/pins/order` is **atomic** — the underlying
//!   [`Store::reorder_pins`] applies the rewrite in one transaction
//!   so readers can never observe a partial reorder (§6.3 row-by-row
//!   note about why `(user_id, position)` is NOT a DB-level unique
//!   constraint).
//! * Every mutating route writes one audit row through
//!   [`crate::audit::record`] — verbs [`audit::PIN_ADD`],
//!   [`audit::PIN_REMOVE`], [`audit::PIN_REORDER`] (SCOPE-PROJECTS
//!   §6.5). The row is written **after** the mutation succeeds; a
//!   store failure leaves no audit trail (matches the `home_org.set`
//!   pattern in [`crate::directory`]).
//!
//! Pins are not a report dimension (§6.2) and therefore do not appear
//! in the §15.6 envelope. The sidebar render cap, the §13.5 tag-link
//! warning, and the §7 tag surface are *not* this stage's concern.
//!
//! [`Principal::actor_user_id`]: crate::audit::Principal::actor_user_id
//! [`Store::reorder_pins`]: dp_domain::store::Store::reorder_pins

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use dp_domain::pin::{Pin, PinKind};
use dp_domain::store::StoreError;

use crate::audit::{self, Principal};
use crate::directory::Ack;
use crate::error::ApiError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Per-user pin cap (SCOPE-PROJECTS §13.5).
// ---------------------------------------------------------------------------

/// Per-user pin cap (data-model cap, §13.5).
///
/// Re-exported from `dp_domain::PIN_CAP` so the REST layer and the
/// Postgres `Store` impl read the *same* constant — the SCOPE-
/// PROJECTS §13.5 working assumption of 20 pins per user. The
/// separate render cap (50 sidebar entries after tag expansion) is
/// a frontend concern and not mirrored here. Eventually moves into
/// `dp-config` per §13.5.
pub use dp_domain::PIN_CAP;

// ---------------------------------------------------------------------------
// Wire DTOs
// ---------------------------------------------------------------------------

/// Wire form of [`PinKind`]. Mirrors the lower-case strings in the
/// `dp_user_pins.kind` CHECK constraint (`'repo' | 'tag'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PinKindDto {
    /// `target_id` references `dp_repos.id`.
    Repo,
    /// `target_id` references `dp_tags.id`.
    Tag,
}

impl From<PinKind> for PinKindDto {
    fn from(k: PinKind) -> Self {
        match k {
            PinKind::Repo => PinKindDto::Repo,
            PinKind::Tag => PinKindDto::Tag,
        }
    }
}

impl From<PinKindDto> for PinKind {
    fn from(k: PinKindDto) -> Self {
        match k {
            PinKindDto::Repo => PinKind::Repo,
            PinKindDto::Tag => PinKind::Tag,
        }
    }
}

/// One row in `GET /me/pins`. Hydration of `target_id` to a full
/// repo / tag payload is a follow-up stage — for now the client
/// joins against `/orgs`, `/repos`, `/tags` itself.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PinDto {
    /// Discriminator for `target_id`.
    pub kind: PinKindDto,
    /// Either a `dp_repos.id` or a `dp_tags.id` per [`kind`](Self::kind).
    pub target_id: Uuid,
    /// Sidebar order. Lower comes first; assigned by the server.
    pub position: i32,
    /// When the row was created.
    pub pinned_at: DateTime<Utc>,
}

impl From<Pin> for PinDto {
    fn from(p: Pin) -> Self {
        Self {
            kind: p.kind.into(),
            target_id: p.target_id,
            position: p.position,
            pinned_at: p.pinned_at,
        }
    }
}

/// Body for `POST /me/pins`. The server picks the position
/// (`len(existing)`) and the `pinned_at` timestamp.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct AddPinRequest {
    /// Pin target kind.
    pub kind: PinKindDto,
    /// `dp_repos.id` or `dp_tags.id` per [`kind`](Self::kind).
    pub target_id: Uuid,
}

/// One entry of [`ReorderRequest::order`].
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct PinKeyDto {
    /// Pin target kind.
    pub kind: PinKindDto,
    /// `dp_repos.id` or `dp_tags.id` per [`kind`](Self::kind).
    pub target_id: Uuid,
}

/// Body for `PUT /me/pins/order`. The slice **must** exactly cover
/// the caller's current pin set — extra / missing entries are
/// rejected (`400 reorder_set_mismatch`).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct ReorderRequest {
    /// New order. Entry `i` becomes `position = i` after the
    /// transaction commits.
    pub order: Vec<PinKeyDto>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /me/pins` — caller's pins, ordered by `position` ascending.
///
/// Audit: not audited. Pins are personal UI state and `GET` traffic
/// would swamp the audit log without operational value (§6.5 only
/// pins the three mutation verbs).
#[utoipa::path(
    get,
    path = "/me/pins",
    responses(
        (status = 200, description = "Caller's pins, ordered by position", body = Vec<PinDto>),
    ),
    tag = "pins",
)]
pub async fn list_pins(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Vec<PinDto>>, ApiError> {
    let pins = state.store.list_pins_for_user(principal.actor_user_id).await?;
    Ok(Json(pins.into_iter().map(PinDto::from).collect()))
}

/// `POST /me/pins` — append a pin to the caller's list.
///
/// * Position is server-assigned (`len(existing)`).
/// * Returns `400 pin_cap_exceeded` if the caller already has
///   [`PIN_CAP`] pins (§13.5).
/// * Returns `409 pin_exists` if the `(kind, target_id)` row is
///   already in the caller's pins (idempotent re-pinning, §6.4 /
///   the composite-PK note in [`dp_domain::store::Store::add_pin`]).
/// * Audit: writes [`audit::PIN_ADD`] with target
///   `"<kind>:<target_id>"` after the row lands.
#[utoipa::path(
    post,
    path = "/me/pins",
    request_body = AddPinRequest,
    responses(
        (status = 200, description = "Pin appended", body = PinDto),
        (status = 400, description = "Pin cap exceeded"),
        (status = 409, description = "Pin already exists for this caller"),
    ),
    tag = "pins",
)]
pub async fn add_pin(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<AddPinRequest>,
) -> Result<Json<PinDto>, ApiError> {
    let user_id = principal.actor_user_id;
    let existing = state.store.list_pins_for_user(user_id).await?;
    if existing.len() >= PIN_CAP {
        return Err(ApiError::BadRequest {
            code: "pin_cap_exceeded",
            message: format!(
                "pin cap of {PIN_CAP} reached; remove an existing pin before adding another"
            ),
        });
    }
    let pin = Pin {
        user_id,
        kind: body.kind.into(),
        target_id: body.target_id,
        position: existing.len() as i32,
        pinned_at: Utc::now(),
    };
    let saved = match state.store.add_pin(&pin).await {
        Ok(p) => p,
        Err(StoreError::Conflict(msg)) => {
            return Err(ApiError::Conflict {
                code: "pin_exists",
                message: format!("pin already exists: {msg}"),
            });
        }
        Err(StoreError::Invalid(msg)) if msg.contains("cap") => {
            return Err(ApiError::BadRequest {
                code: "pin_cap_exceeded",
                message: msg,
            });
        }
        Err(e) => return Err(e.into()),
    };
    audit::record(
        state.store.as_ref(),
        user_id,
        audit::PIN_ADD,
        format!("{}:{}", saved.kind.as_str(), saved.target_id),
    )
    .await?;
    Ok(Json(saved.into()))
}

/// `DELETE /me/pins/{kind}/{id}` — remove one pin.
///
/// Returns `404 pin_not_found` when the row is not in the caller's
/// pin set. Audit: writes [`audit::PIN_REMOVE`] after the row is
/// gone.
///
/// Note on positions: the §6.3 schema does not auto-compact `position`
/// values on delete (the store does not renumber). The next
/// [`reorder_pins`](crate::pins::reorder_pins) call is what compacts
/// the gap.
#[utoipa::path(
    delete,
    path = "/me/pins/{kind}/{target_id}",
    params(
        ("kind"      = PinKindDto, Path, description = "`repo` or `tag`"),
        ("target_id" = Uuid,       Path, description = "Pin target id"),
    ),
    responses(
        (status = 200, description = "Pin removed", body = Ack),
        (status = 404, description = "No such pin for caller"),
    ),
    tag = "pins",
)]
pub async fn remove_pin(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((kind, target_id)): Path<(PinKindDto, Uuid)>,
) -> Result<Json<Ack>, ApiError> {
    let user_id = principal.actor_user_id;
    let dom_kind: PinKind = kind.into();
    match state.store.remove_pin(user_id, dom_kind, target_id).await {
        Ok(()) => {}
        Err(StoreError::NotFound { .. }) => {
            return Err(ApiError::NotFound {
                code: "pin_not_found",
                message: format!("no pin for {}:{}", dom_kind.as_str(), target_id),
            });
        }
        Err(e) => return Err(e.into()),
    }
    audit::record(
        state.store.as_ref(),
        user_id,
        audit::PIN_REMOVE,
        format!("{}:{}", dom_kind.as_str(), target_id),
    )
    .await?;
    Ok(Json(Ack { ok: true }))
}

/// `PUT /me/pins/order` — atomically rewrite the caller's pin order.
///
/// The request body's `order` must exactly cover the caller's
/// current pin set; missing or extra `(kind, target_id)` entries
/// surface as `400 reorder_set_mismatch`. The store applies the
/// rewrite in one transaction so a concurrent reader cannot observe
/// a partial reorder (§6.3 row-by-row comment, §13 atomicity rules).
///
/// Audit: writes one [`audit::PIN_REORDER`] row with target
/// `"count:<n>"` (n = number of pins reordered). We do **not** dump
/// the full key list into the audit target — that would balloon the
/// `dp_audit_log.target` column past the small-text comfort zone
/// and the audit verb is meaningful at row granularity (one row =
/// one reorder operation).
#[utoipa::path(
    put,
    path = "/me/pins/order",
    request_body = ReorderRequest,
    responses(
        (status = 200, description = "Pins reordered atomically", body = Ack),
        (status = 400, description = "`order` does not exactly cover the caller's pins"),
    ),
    tag = "pins",
)]
pub async fn reorder_pins(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<ReorderRequest>,
) -> Result<Json<Ack>, ApiError> {
    let user_id = principal.actor_user_id;
    // Pre-validate against the live set so the caller gets a
    // structured `400 reorder_set_mismatch` rather than the
    // generic `StoreError::Invalid` translation. The store also
    // checks (defence-in-depth) so a CLI / MCP caller cannot
    // bypass.
    let existing = state.store.list_pins_for_user(user_id).await?;
    let order: Vec<(PinKind, Uuid)> = body
        .order
        .iter()
        .map(|k| (k.kind.into(), k.target_id))
        .collect();
    if !reorder_matches(&existing, &order) {
        return Err(ApiError::BadRequest {
            code: "reorder_set_mismatch",
            message: format!(
                "order has {} entries; caller has {} pins; the two sets must match exactly",
                order.len(),
                existing.len()
            ),
        });
    }
    match state.store.reorder_pins(user_id, &order).await {
        Ok(()) => {}
        Err(StoreError::Invalid(msg)) => {
            return Err(ApiError::BadRequest {
                code: "reorder_set_mismatch",
                message: msg,
            });
        }
        Err(e) => return Err(e.into()),
    }
    audit::record(
        state.store.as_ref(),
        user_id,
        audit::PIN_REORDER,
        format!("count:{}", order.len()),
    )
    .await?;
    Ok(Json(Ack { ok: true }))
}

/// Cheap O(n²) set-equality check — `n` is bounded by [`PIN_CAP`]
/// (20) so the constant factors don't matter. Sorts both sides into
/// a stable order then compares.
fn reorder_matches(existing: &[Pin], order: &[(PinKind, Uuid)]) -> bool {
    if existing.len() != order.len() {
        return false;
    }
    let mut a: Vec<(PinKind, Uuid)> =
        existing.iter().map(|p| (p.kind, p.target_id)).collect();
    let mut b: Vec<(PinKind, Uuid)> = order.to_vec();
    a.sort_by_key(|(k, t)| (k.as_str(), *t));
    b.sort_by_key(|(k, t)| (k.as_str(), *t));
    a == b
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the pins router fragment. Mount via `Router::merge` from
/// `dp-server::build`; the composition layer wires the principal
/// extension and the `with_permission` middleware sees the
/// `(pins, <action>)` pair this fragment registers.
pub fn pins_router(state: Arc<AppState>) -> Router {
    use starter_authz::with_permission;
    let inner: AppState = (*state).clone();
    // `pins.read` covers `GET /me/pins`; `pins.write` covers all
    // three mutating routes. The §6.5 audit vocabulary is finer-
    // grained than the authz vocabulary on purpose — authz gates
    // the *capability* (can this principal touch their own pins?)
    // while audit records the *operation* (which one did they do?).
    Router::new()
        .merge(with_permission(
            Router::new().route("/me/pins", get(list_pins)),
            "pins",
            "read",
        ))
        .merge(with_permission(
            Router::new()
                .route("/me/pins", post(add_pin))
                .route("/me/pins/{kind}/{target_id}", delete(remove_pin))
                .route("/me/pins/order", put(reorder_pins)),
            "pins",
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
    use dp_domain::store::{EventActorRow, Store, StoreError};
    use dp_domain::{
        ActivityEvent, ActorRole, EventActor, FetchCursor, FetchRun, FetchRunKind, Membership,
        Org, Repo, ResourceKind, Team, User, WebhookDelivery, Window,
    };

    // -----------------------------------------------------------------
    // In-memory store fake — minimal surface to drive the pins routes
    // -----------------------------------------------------------------

    #[derive(Default)]
    struct MemStore {
        pins: Mutex<Vec<Pin>>,
        audit: Mutex<Vec<AuditEntry>>,
    }

    impl MemStore {
        fn audit_rows(&self) -> Vec<AuditEntry> {
            self.audit.lock().unwrap().clone()
        }
        fn pin_count_for(&self, user_id: Uuid) -> usize {
            self.pins
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.user_id == user_id)
                .count()
        }
        fn pins_for(&self, user_id: Uuid) -> Vec<Pin> {
            let mut out: Vec<Pin> = self
                .pins
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.user_id == user_id)
                .cloned()
                .collect();
            out.sort_by_key(|p| p.position);
            out
        }
    }

    #[async_trait]
    impl Store for MemStore {
        // --- only the methods the pins router touches are implemented
        async fn list_pins_for_user(&self, user_id: Uuid) -> Result<Vec<Pin>, StoreError> {
            Ok(self.pins_for(user_id))
        }
        async fn add_pin(&self, pin: &Pin) -> Result<Pin, StoreError> {
            let mut pins = self.pins.lock().unwrap();
            // Cap defence-in-depth (mirrors the PgStore impl).
            let live = pins.iter().filter(|p| p.user_id == pin.user_id).count();
            if live >= PIN_CAP {
                return Err(StoreError::Invalid(format!(
                    "pin cap of {PIN_CAP} reached"
                )));
            }
            // Composite-PK uniqueness.
            if pins.iter().any(|p| {
                p.user_id == pin.user_id && p.kind == pin.kind && p.target_id == pin.target_id
            }) {
                return Err(StoreError::Conflict("dp_user_pins pkey".into()));
            }
            pins.push(pin.clone());
            Ok(pin.clone())
        }
        async fn remove_pin(
            &self,
            user_id: Uuid,
            kind: PinKind,
            target_id: Uuid,
        ) -> Result<(), StoreError> {
            let mut pins = self.pins.lock().unwrap();
            let before = pins.len();
            pins.retain(|p| {
                !(p.user_id == user_id && p.kind == kind && p.target_id == target_id)
            });
            if pins.len() == before {
                return Err(StoreError::NotFound {
                    entity: "user_pin",
                    id: format!("{}:{}:{}", user_id, kind.as_str(), target_id),
                });
            }
            Ok(())
        }
        async fn reorder_pins(
            &self,
            user_id: Uuid,
            order: &[(PinKind, Uuid)],
        ) -> Result<(), StoreError> {
            let mut pins = self.pins.lock().unwrap();
            // Defence-in-depth set-equality check.
            let live: std::collections::HashSet<(PinKind, Uuid)> = pins
                .iter()
                .filter(|p| p.user_id == user_id)
                .map(|p| (p.kind, p.target_id))
                .collect();
            let new: std::collections::HashSet<(PinKind, Uuid)> =
                order.iter().copied().collect();
            if live != new {
                return Err(StoreError::Invalid(
                    "reorder set does not match live pins".into(),
                ));
            }
            // Apply the rewrite — atomic w.r.t. readers because we
            // hold the mutex.
            for p in pins.iter_mut().filter(|p| p.user_id == user_id) {
                let pos = order
                    .iter()
                    .position(|(k, t)| *k == p.kind && *t == p.target_id)
                    .unwrap() as i32;
                p.position = pos;
            }
            Ok(())
        }
        async fn record_audit_log(&self, entry: &AuditEntry) -> Result<(), StoreError> {
            self.audit.lock().unwrap().push(entry.clone());
            Ok(())
        }

        // --- the rest are minimal stubs ---------------------------------
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
        async fn list_memberships_for_user(
            &self,
            _: Uuid,
        ) -> Result<Vec<Membership>, StoreError> {
            Ok(vec![])
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
    // Test harness — build the router with a Principal extension and
    // a NoopPolicyEngine so the permission middleware always allows.
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
            extra: serde_json::Value::Null,
        };
        pins_router(app_state)
            .layer(Extension(Principal { actor_user_id: actor }))
            .layer(Extension(spi_principal))
            .layer(Extension(engine))
    }

    async fn json_of(resp: axum::response::Response) -> serde_json::Value {
        let body = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn seed_pin(store: &MemStore, user: Uuid, kind: PinKind, target: Uuid, pos: i32) {
        store.pins.lock().unwrap().push(Pin {
            user_id: user,
            kind,
            target_id: target,
            position: pos,
            pinned_at: Utc::now(),
        });
    }

    // -----------------------------------------------------------------
    // GET /me/pins
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn list_pins_returns_caller_rows_in_position_order() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let other = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();
        // Out of insertion order on purpose — handler must sort by position.
        seed_pin(&store, actor, PinKind::Repo, t2, 1);
        seed_pin(&store, actor, PinKind::Tag, t1, 0);
        // Another user's pin must NOT leak.
        seed_pin(&store, other, PinKind::Repo, Uuid::new_v4(), 0);

        let app = build_app(store, actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/me/pins")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2, "only caller's pins");
        assert_eq!(arr[0]["position"], 0);
        assert_eq!(arr[0]["kind"], "tag");
        assert_eq!(arr[1]["position"], 1);
        assert_eq!(arr[1]["kind"], "repo");
    }

    // -----------------------------------------------------------------
    // POST /me/pins
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn add_pin_appends_at_end_and_audits() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        seed_pin(&store, actor, PinKind::Repo, Uuid::new_v4(), 0);
        seed_pin(&store, actor, PinKind::Repo, Uuid::new_v4(), 1);
        let app = build_app(store.clone(), actor);
        let target = Uuid::new_v4();
        let body = serde_json::json!({ "kind": "tag", "target_id": target });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/me/pins")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let v = json_of(resp).await;
        assert_eq!(v["kind"], "tag");
        assert_eq!(v["target_id"], serde_json::json!(target));
        assert_eq!(v["position"], 2, "newly-added pins go to the end");
        assert_eq!(store.pin_count_for(actor), 3);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PIN_ADD);
        assert_eq!(rows[0].target, format!("tag:{target}"));
        assert_eq!(rows[0].actor_user_id, actor);
    }

    #[tokio::test]
    async fn add_pin_rejects_over_cap_with_400() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        for i in 0..PIN_CAP {
            seed_pin(&store, actor, PinKind::Repo, Uuid::new_v4(), i as i32);
        }
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "kind": "repo", "target_id": Uuid::new_v4() });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/me/pins")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "pin_cap_exceeded");
        // Cap-rejected adds leave no audit trail (mutation never landed).
        assert!(store.audit_rows().is_empty());
    }

    #[tokio::test]
    async fn add_pin_rejects_duplicate_with_409() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        seed_pin(&store, actor, PinKind::Repo, target, 0);
        let app = build_app(store.clone(), actor);
        let body = serde_json::json!({ "kind": "repo", "target_id": target });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/me/pins")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "pin_exists");
        assert!(store.audit_rows().is_empty());
    }

    // -----------------------------------------------------------------
    // DELETE /me/pins/{kind}/{id}
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn remove_pin_drops_row_and_audits() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let target = Uuid::new_v4();
        seed_pin(&store, actor, PinKind::Tag, target, 0);
        let app = build_app(store.clone(), actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/me/pins/tag/{target}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(store.pin_count_for(actor), 0);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PIN_REMOVE);
        assert_eq!(rows[0].target, format!("tag:{target}"));
    }

    #[tokio::test]
    async fn remove_pin_returns_404_when_missing_and_no_audit() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let app = build_app(store.clone(), actor);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/me/pins/repo/{}", Uuid::new_v4()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "pin_not_found");
        assert!(store.audit_rows().is_empty());
    }

    // -----------------------------------------------------------------
    // PUT /me/pins/order
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn reorder_pins_rewrites_positions_atomically() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        seed_pin(&store, actor, PinKind::Repo, a, 0);
        seed_pin(&store, actor, PinKind::Repo, b, 1);
        seed_pin(&store, actor, PinKind::Tag, c, 2);
        let app = build_app(store.clone(), actor);
        // Reverse the order: c, b, a.
        let body = serde_json::json!({
            "order": [
                { "kind": "tag",  "target_id": c },
                { "kind": "repo", "target_id": b },
                { "kind": "repo", "target_id": a },
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/me/pins/order")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let pins = store.pins_for(actor);
        assert_eq!(pins[0].target_id, c);
        assert_eq!(pins[0].position, 0);
        assert_eq!(pins[1].target_id, b);
        assert_eq!(pins[1].position, 1);
        assert_eq!(pins[2].target_id, a);
        assert_eq!(pins[2].position, 2);
        let rows = store.audit_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].action, audit::PIN_REORDER);
        assert_eq!(rows[0].target, "count:3");
    }

    #[tokio::test]
    async fn reorder_pins_rejects_set_mismatch_with_400() {
        let store = Arc::new(MemStore::default());
        let actor = Uuid::new_v4();
        let a = Uuid::new_v4();
        seed_pin(&store, actor, PinKind::Repo, a, 0);
        let app = build_app(store.clone(), actor);
        // `order` is missing the existing pin and contains a phantom one.
        let body = serde_json::json!({
            "order": [
                { "kind": "repo", "target_id": Uuid::new_v4() }
            ]
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/me/pins/order")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = json_of(resp).await;
        assert_eq!(v["code"], "reorder_set_mismatch");
        // Positions on the live row are untouched.
        let pins = store.pins_for(actor);
        assert_eq!(pins[0].position, 0);
        assert!(store.audit_rows().is_empty());
    }
}
