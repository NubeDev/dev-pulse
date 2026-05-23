
use dp_domain::membership::Membership;
use dp_domain::org::Org;
use dp_domain::store::StoreError;
use dp_domain::team::Team;
use dp_domain::user::User;
use uuid::Uuid;

use crate::encode::membership_role_to_text;

use super::{map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn upsert_org_impl(&self, org: &Org) -> Result<Org, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_orgs (id, github_id, login, name) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (github_id) DO UPDATE SET \
                 login = EXCLUDED.login, \
                 name  = EXCLUDED.name \
             RETURNING id, github_id, login, name",
        )
        .bind(org.id)
        .bind(org.github_id)
        .bind(&org.login)
        .bind(&org.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_org(&row)
    }

    pub(super) async fn upsert_team_impl(&self, team: &Team) -> Result<Team, StoreError> {
        let row = sqlx::query(
            "INSERT INTO dp_teams (id, org_id, github_id, slug, name) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (org_id, github_id) DO UPDATE SET \
                 slug = EXCLUDED.slug, \
                 name = EXCLUDED.name \
             RETURNING id, org_id, github_id, slug, name",
        )
        .bind(team.id)
        .bind(team.org_id)
        .bind(team.github_id)
        .bind(&team.slug)
        .bind(&team.name)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_team(&row)
    }

    pub(super) async fn upsert_membership_impl(&self, membership: &Membership) -> Result<Membership, StoreError> {
        // home_org intentionally NOT clobbered — only `set_home_org`
        // writes it (TODO §0.5 / SCOPE §3 manual mapping).
        let role_text = membership_role_to_text(&membership.role).to_string();
        let row = sqlx::query(
            "INSERT INTO dp_memberships (user_id, org_id, role, home_org, joined_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (user_id, org_id) DO UPDATE SET \
                 role      = EXCLUDED.role, \
                 home_org  = COALESCE(EXCLUDED.home_org, dp_memberships.home_org), \
                 joined_at = LEAST(dp_memberships.joined_at, EXCLUDED.joined_at) \
             RETURNING user_id, org_id, role, home_org, joined_at",
        )
        .bind(membership.user_id)
        .bind(membership.org_id)
        .bind(&role_text)
        .bind(membership.home_org)
        .bind(membership.joined_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_membership(&row)
    }

    pub(super) async fn list_memberships_for_user_impl(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query(
            "SELECT user_id, org_id, role, home_org, joined_at \
             FROM dp_memberships WHERE user_id = $1 ORDER BY org_id",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_membership).collect()
    }

    pub(super) async fn set_home_org_impl(
        &self,
        user_id: Uuid,
        org_id: Uuid,
        home_org: Option<Uuid>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE dp_memberships SET home_org = $3 \
             WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .bind(home_org)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("membership", format!("({user_id}, {org_id})")));
        }
        Ok(())
    }

    pub(super) async fn set_home_org_for_user_impl(
        &self,
        user_id: Uuid,
        org_id: Uuid,
    ) -> Result<(), StoreError> {
        // One transaction: clear every other home_org for this user
        // and set the (user, org_id) row in one shot so a concurrent
        // reader cannot observe two home_org=Some rows. The single
        // statement uses a CASE expression keyed on org_id; the
        // ROW_COUNT after execution tells us whether the target row
        // existed at all (we look it up explicitly so the error path
        // mirrors set_home_org).
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT user_id FROM dp_memberships \
             WHERE user_id = $1 AND org_id = $2",
        )
        .bind(user_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if exists.is_none() {
            return Err(not_found("membership", format!("({user_id}, {org_id})")));
        }
        sqlx::query(
            "UPDATE dp_memberships \
             SET home_org = CASE WHEN org_id = $2 THEN $2 ELSE NULL END \
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn list_orgs_impl(&self) -> Result<Vec<Org>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, github_id, login, name FROM dp_orgs ORDER BY login",
        )
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_org).collect()
    }

    pub(super) async fn list_teams_for_org_impl(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, org_id, github_id, slug, name \
             FROM dp_teams WHERE org_id = $1 ORDER BY slug",
        )
        .bind(org_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_team).collect()
    }

    pub(super) async fn list_users_for_org_impl(&self, org_id: Uuid) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query(
            "SELECT u.id, u.github_id, u.login, u.email, u.name, u.deleted_at \
             FROM dp_users u \
             JOIN dp_memberships m ON m.user_id = u.id \
             WHERE m.org_id = $1 AND u.deleted_at IS NULL \
             ORDER BY u.login",
        )
        .bind(org_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user).collect()
    }
}
