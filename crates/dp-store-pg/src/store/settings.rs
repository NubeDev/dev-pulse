
use dp_domain::setting::UserSetting;
use dp_domain::store::StoreError;
use uuid::Uuid;


use super::{map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn list_user_settings_impl(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserSetting>, StoreError> {
        let rows = sqlx::query(
            "SELECT user_id, key, value, is_secret, updated_at \
             FROM dp_user_settings WHERE user_id = $1 ORDER BY key ASC",
        )
        .bind(user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_user_setting).collect()
    }

    pub(super) async fn get_user_setting_impl(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<Option<UserSetting>, StoreError> {
        let row = sqlx::query(
            "SELECT user_id, key, value, is_secret, updated_at \
             FROM dp_user_settings WHERE user_id = $1 AND key = $2",
        )
        .bind(user_id)
        .bind(key)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_user_setting).transpose()
    }

    pub(super) async fn upsert_user_setting_impl(
        &self,
        setting: &UserSetting,
    ) -> Result<UserSetting, StoreError> {
        // Upsert: same (user_id, key) replaces value + flips
        // is_secret + stamps updated_at. updated_at is bumped
        // server-side so the caller can't backdate writes.
        let row = sqlx::query(
            "INSERT INTO dp_user_settings \
                 (user_id, key, value, is_secret, updated_at) \
             VALUES ($1, $2, $3, $4, now()) \
             ON CONFLICT (user_id, key) DO UPDATE \
             SET value = EXCLUDED.value, \
                 is_secret = EXCLUDED.is_secret, \
                 updated_at = now() \
             RETURNING user_id, key, value, is_secret, updated_at",
        )
        .bind(setting.user_id)
        .bind(&setting.key)
        .bind(&setting.value)
        .bind(setting.is_secret)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_user_setting(&row)
    }

    pub(super) async fn delete_user_setting_impl(
        &self,
        user_id: Uuid,
        key: &str,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            "DELETE FROM dp_user_settings \
             WHERE user_id = $1 AND key = $2",
        )
        .bind(user_id)
        .bind(key)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("user_setting", key));
        }
        Ok(())
    }
}
