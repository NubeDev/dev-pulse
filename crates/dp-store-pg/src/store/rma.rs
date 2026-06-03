//! Store impls for returns / RMA (DOCS/ideas/product-manufacturing.md
//! §5.5).
//!
//! An RMA is product-scoped history with an optional serialised unit.
//! Two invariants matter on the write path (§8):
//!
//! * The named product must exist (else `NotFound`).
//! * If a `unit_id` is supplied it must belong to that product (else
//!   `Invalid`) — an RMA can't point at a unit of a different product.
//!
//! The unique index on `(org_id, lower(rma_number))` surfaces as
//! `StoreError::Conflict` via `map_sqlx`; the `version` CAS update
//! disambiguates a miss into `Conflict` (stale) vs `NotFound` (gone).

use dp_domain::rma::{Rma, RmaCreate, RmaFilter, RmaUpdate};
use dp_domain::store::StoreError;
use uuid::Uuid;

use super::rows::row_to_rma;
use super::{map_sqlx, not_found, PgStore};

const RMA_COLS: &str = "id, org_id, unit_id, product_id, customer_id, rma_number, \
    under_warranty, status, reason, diagnosis, resolution, received_at, resolved_at, \
    created_by, created_at, updated_at, version";

impl PgStore {
    pub(super) async fn list_rma_impl(
        &self,
        filter: &RmaFilter,
    ) -> Result<Vec<Rma>, StoreError> {
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let sql = format!(
            r#"SELECT {RMA_COLS}
                 FROM dp_rma_returns
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::uuid IS NULL OR product_id = $3)
                  AND ($4::uuid IS NULL OR customer_id = $4)
                  AND ($5::uuid IS NULL OR unit_id = $5)
             ORDER BY created_at DESC"#
        );
        let rows = sqlx::query(&sql)
            .bind(filter.org_id)
            .bind(status_text)
            .bind(filter.product_id)
            .bind(filter.customer_id)
            .bind(filter.unit_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_rma).collect()
    }

    pub(super) async fn get_rma_impl(&self, id: Uuid) -> Result<Option<Rma>, StoreError> {
        let sql = format!("SELECT {RMA_COLS} FROM dp_rma_returns WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row.as_ref().map(row_to_rma).transpose()
    }

    pub(super) async fn create_rma_impl(&self, c: &RmaCreate) -> Result<Rma, StoreError> {
        // Parent-child validation (§8): the product must exist, and any
        // supplied unit must belong to that product.
        let product_exists = sqlx::query_scalar::<_, i32>("SELECT 1 FROM dp_products WHERE id = $1")
            .bind(c.product_id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?
            .is_some();
        if !product_exists {
            return Err(not_found("product", c.product_id));
        }
        if let Some(unit_id) = c.unit_id {
            let unit_product: Option<Uuid> =
                sqlx::query_scalar("SELECT product_id FROM dp_product_units WHERE id = $1")
                    .bind(unit_id)
                    .fetch_optional(self.pool.sqlx())
                    .await
                    .map_err(map_sqlx)?;
            match unit_product {
                None => return Err(not_found("unit", unit_id)),
                Some(pid) if pid != c.product_id => {
                    return Err(StoreError::Invalid(
                        "unit does not belong to product".into(),
                    ));
                }
                Some(_) => {}
            }
        }
        let sql = format!(
            r#"INSERT INTO dp_rma_returns
                   (org_id, unit_id, product_id, customer_id, rma_number, under_warranty,
                    status, reason, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING {RMA_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(c.org_id)
            .bind(c.unit_id)
            .bind(c.product_id)
            .bind(c.customer_id)
            .bind(&c.rma_number)
            .bind(c.under_warranty)
            .bind(c.status.as_str())
            .bind(c.reason.as_deref())
            .bind(c.created_by)
            .fetch_one(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row_to_rma(&row)
    }

    pub(super) async fn update_rma_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &RmaUpdate,
    ) -> Result<Rma, StoreError> {
        // If the patch (re)links a unit, re-validate it belongs to this
        // RMA's product (§8).
        if let Some(unit_id) = u.unit_id {
            let cur = self.get_rma_impl(id).await?.ok_or_else(|| not_found("rma", id))?;
            let unit_product: Option<Uuid> =
                sqlx::query_scalar("SELECT product_id FROM dp_product_units WHERE id = $1")
                    .bind(unit_id)
                    .fetch_optional(self.pool.sqlx())
                    .await
                    .map_err(map_sqlx)?;
            match unit_product {
                None => return Err(not_found("unit", unit_id)),
                Some(pid) if pid != cur.product_id => {
                    return Err(StoreError::Invalid(
                        "unit does not belong to product".into(),
                    ));
                }
                Some(_) => {}
            }
        }
        let sql = format!(
            r#"UPDATE dp_rma_returns
                  SET unit_id=$3, customer_id=$4, under_warranty=$5, status=$6,
                      reason=$7, diagnosis=$8, resolution=$9, received_at=$10, resolved_at=$11,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {RMA_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .bind(u.unit_id)
            .bind(u.customer_id)
            .bind(u.under_warranty)
            .bind(u.status.as_str())
            .bind(u.reason.as_deref())
            .bind(u.diagnosis.as_deref())
            .bind(u.resolution.as_deref())
            .bind(u.received_at)
            .bind(u.resolved_at)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_rma(&r),
            None => {
                if self.get_rma_impl(id).await?.is_some() {
                    Err(StoreError::Conflict(format!("stale version for rma {id}")))
                } else {
                    Err(not_found("rma", id))
                }
            }
        }
    }
}
