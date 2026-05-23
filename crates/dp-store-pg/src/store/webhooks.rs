
use dp_domain::store::StoreError;
use dp_domain::webhook::WebhookDelivery;
use uuid::Uuid;


use super::{map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn enqueue_webhook_impl(&self, delivery: &WebhookDelivery) -> Result<(), StoreError> {
        // No ON CONFLICT — we WANT the unique-violation on
        // `delivery_id` to surface so the caller can translate it to
        // a 200 OK and avoid double-processing.
        sqlx::query(
            "INSERT INTO dp_webhook_inbox \
                 (id, delivery_id, event, payload, received_at, processed_at, error) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(delivery.id)
        .bind(&delivery.delivery_id)
        .bind(&delivery.event)
        .bind(&delivery.payload)
        .bind(delivery.received_at)
        .bind(delivery.processed_at)
        .bind(&delivery.error)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn claim_webhooks_impl(&self, max: i64) -> Result<Vec<WebhookDelivery>, StoreError> {
        // `FOR UPDATE SKIP LOCKED` is how multiple workers cooperate
        // without serialising — Postgres-canonical queue pattern.
        // The CTE writes the lock; the outer SELECT returns the
        // rows shaped like the regular read.
        let rows = sqlx::query(
            "WITH claimed AS ( \
                 SELECT id FROM dp_webhook_inbox \
                 WHERE processed_at IS NULL \
                 ORDER BY received_at \
                 LIMIT $1 \
                 FOR UPDATE SKIP LOCKED \
             ) \
             SELECT w.id, w.delivery_id, w.event, w.payload, \
                    w.received_at, w.processed_at, w.error \
             FROM dp_webhook_inbox w \
             JOIN claimed c ON c.id = w.id",
        )
        .bind(max)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_webhook_delivery).collect()
    }

    pub(super) async fn mark_webhook_processed_impl(&self, id: Uuid) -> Result<(), StoreError> {
        let result = sqlx::query(
            "UPDATE dp_webhook_inbox SET processed_at = NOW(), error = NULL \
             WHERE id = $1",
        )
        .bind(id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("webhook", id));
        }
        Ok(())
    }

    pub(super) async fn mark_webhook_failed_impl(&self, id: Uuid, error: &str) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE dp_webhook_inbox SET error = $2 WHERE id = $1")
            .bind(id)
            .bind(error)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if result.rows_affected() == 0 {
            return Err(not_found("webhook", id));
        }
        Ok(())
    }
}
