//! Load `starter-authz::StaticRbacEngine` from the dev-pulse
//! policy file + register the resource kinds dp-rest uses.
//!
//! Production wiring (called from [`crate::build`]):
//!
//! ```ignore
//! let registry = std::sync::Arc::new(starter_authz::StaticRegistry::new());
//! crate::auth::register_dev_pulse_resources(&registry);
//! let engine = crate::auth::load_static_engine(
//!     "crates/dp-server/policy/dev-pulse.toml",
//!     registry,
//! )?;
//! // hand `engine` in as AppState.policy
//! ```
//!
//! ## Why the `AwaitingAccessEngine` wrapper
//!
//! `StaticRbacEngine` returns a generic `no_matching_rule` reason
//! when nothing matches; SCOPE D4.2 requires the 403 body to read
//! `{"error":"awaiting_access"}` for an out-of-org GitHub user
//! specifically. We wrap the engine and rewrite the reason at the
//! edge — staying out of `starter-authz`'s internals (the
//! R-no-starter-edit boundary) while honouring the wire contract.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use starter_authz::{AuthzConfig, StaticRbacEngine, StaticRegistry};
use starter_spi::auth::Principal;
use starter_spi::authz::{Decision, Ownership, PolicyEngine, ResourceRef, ResourceSpec};
use thiserror::Error;

/// The deny reason returned to clients when the org-gate rule
/// fails (out-of-org user). Pinned constant so the smoke test and
/// the policy docs reference one source of truth.
pub const AWAITING_ACCESS_REASON: &str = "awaiting_access";

/// Errors when loading the policy file.
#[derive(Debug, Error)]
pub enum PolicyLoadError {
    /// Reading or parsing the TOML failed.
    #[error("policy load: {0}")]
    Config(String),
    /// `StaticRbacEngine::from_config` rejected the compiled
    /// ruleset (e.g. an unknown resource referenced by a rule).
    #[error("policy compile: {0}")]
    Compile(String),
}

/// Register every resource kind dp-rest's `require_permission`
/// decorations reference. The kinds are pinned here (not derived
/// dynamically) so adding a new protected route is a deliberate
/// two-line edit — one `register_spec(...)` here, one
/// `.layer(require_permission(...))` on the route — and the
/// boundary smoke catches drift.
///
/// `Ownership::None` everywhere: dev-pulse's row-level rules are
/// the home-org membership check (D4.4 `home_org.set`), not a
/// `principal.subject == row.owner` flow.
pub fn register_dev_pulse_resources(registry: &StaticRegistry) {
    registry.register_spec(ResourceSpec::from_static(
        "reports",
        &["read"],
        Ownership::None,
        "Reports",
        "Aggregated activity-event reports per user, team, org, or home-org split.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "users",
        &["read"],
        Ownership::None,
        "Users",
        "User directory listing.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "orgs",
        &["read"],
        Ownership::None,
        "Orgs",
        "Org directory listing.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "teams",
        &["read"],
        Ownership::None,
        "Teams",
        "Team directory listing.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "home_org",
        &["set"],
        Ownership::None,
        "Home org",
        "Per-user home-org assignment (memberships.home_org flip).",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "admin",
        &["read", "refresh", "anonymise", "export"],
        Ownership::None,
        "Admin",
        "Admin surface: reconciler runs, refresh trigger, GDPR anonymise/export.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "pins",
        &["read", "write"],
        Ownership::None,
        "Pins",
        "Per-user pinned repos / tags (SCOPE-PROJECTS §6). `write` covers add / remove / reorder.",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "github_app",
        &["read"],
        Ownership::None,
        "GitHub App",
        "Per-viewer GitHub App install permission surface (SCOPE-PROJECTS §8.4, §13.6). \
         `read` covers `GET /me/app-install-banner`; the §8.4 write-gate runs inside \
         issue-mutation handlers (gated under their own resource).",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "issues",
        &["read", "write"],
        Ownership::None,
        "Issues",
        "Issue read surface (`/issues`, `/issues/{id}`, `/me/queue`, inbox state) \
         and the §8 write surface (create / patch / comment).",
    ));
    registry.register_spec(ResourceSpec::from_static(
        "tags",
        &["read", "write"],
        Ownership::None,
        "Tags",
        "Per-user / shared tag taxonomy (SCOPE-PROJECTS §6). `write` covers \
         create / rename / delete / assignment.",
    ));
}

/// Load + compile the policy file, wrap it in
/// [`AwaitingAccessEngine`], and hand back an
/// `Arc<dyn PolicyEngine>` that [`crate::AppState`] can carry.
///
/// `path` is the on-disk TOML; the production caller passes
/// `crates/dp-server/policy/dev-pulse.toml`. Tests can build
/// configs in-memory via [`load_static_engine_from_config`].
pub fn load_static_engine(
    path: impl AsRef<Path>,
    registry: Arc<StaticRegistry>,
) -> Result<Arc<dyn PolicyEngine>, PolicyLoadError> {
    let cfg = AuthzConfig::from_path(path).map_err(|e| PolicyLoadError::Config(e.to_string()))?;
    load_static_engine_from_config(cfg, registry)
}

/// Variant that takes an already-parsed [`AuthzConfig`]. Useful
/// for tests and for callers that compose the config from
/// multiple sources.
pub fn load_static_engine_from_config(
    cfg: AuthzConfig,
    registry: Arc<StaticRegistry>,
) -> Result<Arc<dyn PolicyEngine>, PolicyLoadError> {
    let engine =
        StaticRbacEngine::from_config(cfg, registry).map_err(|e| PolicyLoadError::Compile(e.to_string()))?;
    Ok(Arc::new(AwaitingAccessEngine {
        inner: Arc::new(engine),
    }))
}

/// `PolicyEngine` decorator that rewrites the generic
/// `no_matching_rule` deny reason into the SCOPE D4.2 stable
/// `awaiting_access` code. Other deny reasons (e.g.
/// `unknown_resource`, `explicit_deny`, `not_owner`) ride through
/// unchanged — they describe distinct misconfigurations the
/// operator should be able to spot in the audit trail.
pub struct AwaitingAccessEngine {
    inner: Arc<dyn PolicyEngine>,
}

impl AwaitingAccessEngine {
    /// Wrap an existing engine.
    pub fn new(inner: Arc<dyn PolicyEngine>) -> Self {
        Self { inner }
    }
}

/// Rule id of the org-gate deny rule in `dev-pulse.toml`. The
/// reason-rewrite below keys on this so a future deny added for
/// a different reason (e.g. an explicit per-user block) keeps its
/// original `explicit_deny` reason instead of being silently
/// re-labelled `awaiting_access`.
pub const ORG_GATE_DENY_RULE_ID: &str = "org-gate-deny-out-of-org";

#[async_trait]
impl PolicyEngine for AwaitingAccessEngine {
    async fn check(
        &self,
        principal: &Principal,
        action: &str,
        object: &ResourceRef,
    ) -> Decision {
        let decision = self.inner.check(principal, action, object).await;
        match decision {
            // Two paths to "not in an allowed org":
            //   1. No rule matched at all (no defaults loaded, no
            //      allow-rule fired) → reason `no_matching_rule`.
            //   2. The org-gate deny rule explicitly matched
            //      because `oauth.in_allowed_org != true` →
            //      reason `explicit_deny` matched by
            //      `org-gate-deny-out-of-org`.
            // Both should surface to the client as the stable
            // `awaiting_access` code per SCOPE D4.2.
            Decision::Deny {
                reason,
                matched_rule,
            } if reason == "no_matching_rule"
                || matched_rule.as_deref() == Some(ORG_GATE_DENY_RULE_ID) =>
            {
                Decision::Deny {
                    reason: AWAITING_ACCESS_REASON.to_string(),
                    matched_rule,
                }
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use starter_spi::auth::Role;

    fn principal(in_allowed: bool) -> Principal {
        Principal {
            subject: "u1".into(),
            role: Role::Reader,
            scopes: Vec::new(),
            extra: json!({
                "oauth": {
                    "github_orgs": if in_allowed { vec!["NubeIO"] } else { vec![] },
                    "in_allowed_org": in_allowed,
                }
            }),
        }
    }

    fn engine() -> Arc<dyn PolicyEngine> {
        let registry = Arc::new(StaticRegistry::new());
        register_dev_pulse_resources(&registry);
        load_static_engine(
            // Repo-root-relative path; cargo runs tests with CWD =
            // crate root, so the file resolves.
            "policy/dev-pulse.toml",
            registry,
        )
        .expect("policy loads")
    }

    #[tokio::test]
    async fn in_allowed_org_user_is_allowed_on_reports_read() {
        let e = engine();
        let d = e
            .check(&principal(true), "read", &ResourceRef::collection("reports"))
            .await;
        assert!(matches!(d, Decision::Allow { .. }), "got {d:?}");
    }

    #[tokio::test]
    async fn out_of_org_user_gets_awaiting_access() {
        let e = engine();
        let d = e
            .check(
                &principal(false),
                "read",
                &ResourceRef::collection("reports"),
            )
            .await;
        match d {
            Decision::Deny { reason, .. } => {
                assert_eq!(
                    reason, AWAITING_ACCESS_REASON,
                    "no_matching_rule must be rewritten"
                );
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_resource_reason_is_preserved() {
        let e = engine();
        let d = e
            .check(
                &principal(true),
                "read",
                &ResourceRef::collection("not_registered"),
            )
            .await;
        match d {
            Decision::Deny { reason, .. } => {
                // Non-no_matching_rule denies are not rewritten —
                // operators need to spot config errors distinctly
                // from policy denials.
                assert_eq!(reason, "unknown_resource");
            }
            _ => panic!("expected deny"),
        }
    }
}
