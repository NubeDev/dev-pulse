
use chrono::{DateTime, Utc};
use dp_domain::identity::{IdentityLinkPending, UserIdentity};
use dp_domain::store::StoreError;
use dp_domain::user::{Role, User};
use uuid::Uuid;


use super::{map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn upsert_user_impl(&self, user: &User) -> Result<User, StoreError> {
        // `role` is intentionally NOT part of `ON CONFLICT DO UPDATE`:
        // the fetcher upserts on every GitHub-side change, and we don't
        // want a re-stamp to clobber the operator-chosen tier.
        // Role mutations go through `set_user_role` only.
        let row = sqlx::query(
            "INSERT INTO dp_users (id, github_id, login, email, name, role, deleted_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (github_id) DO UPDATE SET \
                 login      = EXCLUDED.login, \
                 email      = EXCLUDED.email, \
                 name       = EXCLUDED.name, \
                 deleted_at = EXCLUDED.deleted_at \
             RETURNING id, github_id, login, email, name, role, deleted_at",
        )
        .bind(user.id)
        .bind(user.github_id)
        .bind(&user.login)
        .bind(&user.email)
        .bind(&user.name)
        .bind(user.role.as_str())
        .bind(user.deleted_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user(&row)
    }

    pub(super) async fn get_user_impl(&self, id: Uuid) -> Result<User, StoreError> {
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, role, deleted_at \
             FROM dp_users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", id)),
        }
    }

    pub(super) async fn get_user_by_github_id_impl(&self, github_id: i64) -> Result<User, StoreError> {
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, role, deleted_at \
             FROM dp_users WHERE github_id = $1",
        )
        .bind(github_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", github_id)),
        }
    }

    pub(super) async fn list_users_impl(&self) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, github_id, login, email, name, role, deleted_at \
             FROM dp_users WHERE deleted_at IS NULL ORDER BY login",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user).collect()
    }

    pub(super) async fn find_user_by_login_impl(&self, login: &str) -> Result<Option<User>, StoreError> {
        // Prefer the row with a real (positive) github_id when both
        // a synthetic (negative) trailer row and the real row exist
        // for the same login — the trailer path uses this to fold
        // future events onto the canonical row. Match case-insensitively
        // (GitHub logins are) and prefer the *lowest* positive github_id
        // (oldest real GitHub account) so this agrees with the
        // canonical-row rule in migration 0003.
        let row = sqlx::query(
            "SELECT id, github_id, login, email, name, role, deleted_at \
             FROM dp_users \
             WHERE lower(login) = lower($1) AND deleted_at IS NULL \
             ORDER BY (github_id >= 0) DESC, github_id ASC \
             LIMIT 1",
        )
        .bind(login)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r).map(Some),
            None => Ok(None),
        }
    }

    pub(super) async fn pseudonymise_user_impl(&self, id: Uuid) -> Result<(), StoreError> {
        // Rewrite to a stable `deleted-user-<short-id>` form. The
        // hash is derived from the row id so re-running this is a
        // no-op (idempotent) and two different users never collide.
        let short = id.simple().to_string();
        let short = &short[..16];
        let login = format!("deleted-user-{short}");
        let result = sqlx::query(
            "UPDATE dp_users SET \
                 login      = $2, \
                 email      = NULL, \
                 name       = NULL, \
                 deleted_at = COALESCE(deleted_at, NOW()) \
             WHERE id = $1",
        )
        .bind(id)
        .bind(&login)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("user", id));
        }
        Ok(())
    }

    pub(super) async fn list_identities_for_user_impl(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserIdentity>, StoreError> {
        // Primary first, then newest link first. Ties on linked_at
        // break by github_user_id for a deterministic order under
        // CI fixture clock skew.
        let rows = sqlx::query(
            "SELECT user_id, github_user_id, github_login, is_primary, \
                    linked_at, verified_via \
             FROM dp_user_identities \
             WHERE user_id = $1 \
             ORDER BY is_primary DESC, linked_at DESC, github_user_id ASC",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user_identity).collect()
    }

    pub(super) async fn find_user_by_github_user_id_impl(
        &self,
        github_user_id: i64,
    ) -> Result<Option<User>, StoreError> {
        let row = sqlx::query(
            "SELECT u.id, u.github_id, u.login, u.email, u.name, u.role, u.deleted_at \
             FROM dp_user_identities i \
             JOIN dp_users u ON u.id = i.user_id \
             WHERE i.github_user_id = $1 AND u.deleted_at IS NULL",
        )
        .bind(github_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r).map(Some),
            None => Ok(None),
        }
    }

    pub(super) async fn create_identity_link_pending_impl(
        &self,
        pending: &IdentityLinkPending,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO dp_identity_link_pending \
                 (nonce, dp_user_id, session_id, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(pending.nonce)
        .bind(pending.dp_user_id)
        .bind(&pending.session_id)
        .bind(pending.created_at)
        .bind(pending.expires_at)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn consume_identity_link_pending_impl(
        &self,
        nonce: Uuid,
    ) -> Result<Option<IdentityLinkPending>, StoreError> {
        // RETURNING on DELETE atomically reads + removes the row
        // so a replayed callback cannot consume the same nonce twice.
        let row = sqlx::query(
            "DELETE FROM dp_identity_link_pending \
             WHERE nonce = $1 \
             RETURNING nonce, dp_user_id, session_id, created_at, expires_at",
        )
        .bind(nonce)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => Ok(Some(row_to_identity_link_pending(&r)?)),
            None => Ok(None),
        }
    }

    pub(super) async fn purge_expired_identity_link_pending_impl(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "DELETE FROM dp_identity_link_pending WHERE expires_at < $1",
        )
        .bind(now)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(result.rows_affected())
    }

    pub(super) async fn link_identity_impl(
        &self,
        identity: &UserIdentity,
    ) -> Result<UserIdentity, StoreError> {
        // One transaction so the "first identity is primary"
        // promotion and the insert can never tear: a concurrent
        // writer either sees zero rows (and also becomes primary)
        // or sees the new row.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Ensure the target dp-user actually exists; the FK would
        // catch this too but the NotFound is friendlier.
        let user_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM dp_users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(identity.user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if user_exists.is_none() {
            return Err(not_found("user", identity.user_id));
        }

        // Reject if any other dp-user already claims this
        // github_user_id. We surface a Conflict so the handler can
        // emit IDENTITY_CLAIM_CONFLICT + HTTP 409. (The UNIQUE
        // constraint also catches this on INSERT; checking here
        // makes the error path deterministic regardless of which
        // dp-user wins the race.)
        let claimed_by: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM dp_user_identities WHERE github_user_id = $1",
        )
        .bind(identity.github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if let Some(owner) = claimed_by {
            if owner != identity.user_id {
                return Err(StoreError::Conflict(format!(
                    "github_user_id {} is already claimed by another dp-user",
                    identity.github_user_id
                )));
            }
        }

        // The first identity for a user is always primary, even
        // if the caller passed `is_primary = false`. Otherwise we
        // honour the caller's choice; if they pass `true` we flip
        // every other row for the user to FALSE first to keep the
        // partial unique index happy.
        let existing_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dp_user_identities WHERE user_id = $1",
        )
        .bind(identity.user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let effective_primary = identity.is_primary || existing_count == 0;
        if effective_primary && existing_count > 0 {
            sqlx::query(
                "UPDATE dp_user_identities SET is_primary = FALSE \
                 WHERE user_id = $1 AND is_primary",
            )
            .bind(identity.user_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }

        let row = sqlx::query(
            "INSERT INTO dp_user_identities \
                 (user_id, github_user_id, github_login, is_primary, \
                  linked_at, verified_via) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (user_id, github_user_id) DO UPDATE SET \
                 github_login = EXCLUDED.github_login, \
                 verified_via = EXCLUDED.verified_via \
             RETURNING user_id, github_user_id, github_login, is_primary, \
                       linked_at, verified_via",
        )
        .bind(identity.user_id)
        .bind(identity.github_user_id)
        .bind(&identity.github_login)
        .bind(effective_primary)
        .bind(identity.linked_at)
        .bind(identity.verified_via.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        let out = row_to_user_identity(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(out)
    }

    pub(super) async fn unlink_identity_impl(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Snapshot the row so we can return a useful error and so
        // we know whether it was primary before the delete.
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT is_primary FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2 FOR UPDATE",
        )
        .bind(user_id)
        .bind(github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some((is_primary,)) = row else {
            return Err(not_found("identity", github_user_id));
        };

        // Last identity rule: refuse to leave the user with zero
        // rows. The principal stamper would 401 them on the next
        // request otherwise, which is worse than a clean 4xx here.
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dp_user_identities WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if remaining <= 1 {
            return Err(StoreError::Invalid(
                "cannot unlink the last identity for a user".into(),
            ));
        }
        if is_primary {
            return Err(StoreError::Invalid(
                "cannot unlink the primary identity; set another primary first"
                    .into(),
            ));
        }

        sqlx::query(
            "DELETE FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        // CASCADE has dropped `dp_membership_identities` rows for
        // this `github_user_id`. Collapse any `dp_memberships`
        // rows the user can no longer reach via *any* remaining
        // identity, so the §3.0.2.b invariant holds at commit time.
        sqlx::query(
            "DELETE FROM dp_memberships m \
             WHERE m.user_id = $1 \
               AND NOT EXISTS ( \
                   SELECT 1 FROM dp_membership_identities mi \
                   WHERE mi.user_id = m.user_id AND mi.org_id = m.org_id \
               )",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn set_user_role_impl(
        &self,
        id: Uuid,
        role: Role,
    ) -> Result<User, StoreError> {
        // The CHECK constraint on dp_users.role enforces the
        // closed enum, so we don't double-validate here — the
        // typed `Role` parameter already does that work at the
        // call site.
        let row = sqlx::query(
            "UPDATE dp_users SET role = $2 \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING id, github_id, login, email, name, role, deleted_at",
        )
        .bind(id)
        .bind(role.as_str())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", id)),
        }
    }

    pub(super) async fn update_user_impl(
        &self,
        id: Uuid,
        name: Option<Option<String>>,
        email: Option<Option<String>>,
    ) -> Result<User, StoreError> {
        // COALESCE-free conditional update: each field is only
        // rewritten when the caller passed `Some(..)`. The `$N::bool`
        // flags let a single statement express "leave unchanged" vs
        // "set (possibly to NULL)" without dynamic SQL.
        let (set_name, name_val) = match name {
            Some(v) => (true, v),
            None => (false, None),
        };
        let (set_email, email_val) = match email {
            Some(v) => (true, v),
            None => (false, None),
        };
        let row = sqlx::query(
            "UPDATE dp_users SET \
                 name  = CASE WHEN $2 THEN $3 ELSE name  END, \
                 email = CASE WHEN $4 THEN $5 ELSE email END \
             WHERE id = $1 AND deleted_at IS NULL \
             RETURNING id, github_id, login, email, name, role, deleted_at",
        )
        .bind(id)
        .bind(set_name)
        .bind(name_val)
        .bind(set_email)
        .bind(email_val)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_user(&r),
            None => Err(not_found("user", id)),
        }
    }

    pub(super) async fn set_primary_identity_impl(
        &self,
        user_id: Uuid,
        github_user_id: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM dp_user_identities \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if exists.is_none() {
            return Err(not_found("identity", github_user_id));
        }

        // Demote the current primary, then promote the target. PG
        // would briefly see two `is_primary = TRUE` rows for the
        // same user inside the transaction except the partial
        // unique index is checked at statement end; doing the
        // demote first keeps the index happy on both deferred and
        // immediate constraint modes.
        sqlx::query(
            "UPDATE dp_user_identities SET is_primary = FALSE \
             WHERE user_id = $1 AND is_primary",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        sqlx::query(
            "UPDATE dp_user_identities SET is_primary = TRUE \
             WHERE user_id = $1 AND github_user_id = $2",
        )
        .bind(user_id)
        .bind(github_user_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }
}
