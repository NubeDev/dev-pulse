//! Store impls for per-product software / firmware release history
//! (DOCS/ideas/product-manufacturing.md §5.x).
//!
//! Releases follow the §8.2 CAS contract. Archive is a soft-delete
//! (sets `archived_at`) so the partial-unique `(product, kind, major,
//! minor)` index — keyed on `archived_at IS NULL` — frees the slot for
//! reuse. A duplicate version surfaces as `Conflict` via `map_sqlx`.

use dp_domain::product_release::{
    ProductRelease, ProductReleaseCreate, ProductReleaseUpdate, ReleaseKind,
};
use dp_domain::store::StoreError;
use uuid::Uuid;

use super::rows::row_to_product_release;
use super::{map_sqlx, not_found, PgStore};

const RELEASE_COLS: &str = "id, org_id, product_id, kind, major, minor, release_notes, \
    released_at, links, archived_at, created_by, created_at, updated_at, version";

fn links_json(links: &[dp_domain::product_release::ReleaseLink]) -> serde_json::Value {
    serde_json::to_value(links).unwrap_or_else(|_| serde_json::json!([]))
}

impl PgStore {
    pub(super) async fn list_product_releases_impl(
        &self,
        product_id: Uuid,
        kind: Option<ReleaseKind>,
    ) -> Result<Vec<ProductRelease>, StoreError> {
        let kind_text = kind.map(|k| k.as_str().to_string());
        let sql = format!(
            r#"SELECT {RELEASE_COLS}
                 FROM dp_product_releases
                WHERE product_id = $1
                  AND archived_at IS NULL
                  AND ($2::text IS NULL OR kind = $2)
             ORDER BY kind, major DESC, minor DESC"#
        );
        let rows = sqlx::query(&sql)
            .bind(product_id)
            .bind(kind_text)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_product_release).collect()
    }

    pub(super) async fn get_product_release_impl(
        &self,
        id: Uuid,
    ) -> Result<Option<ProductRelease>, StoreError> {
        let sql = format!("SELECT {RELEASE_COLS} FROM dp_product_releases WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row.as_ref().map(row_to_product_release).transpose()
    }

    pub(super) async fn create_product_release_impl(
        &self,
        c: &ProductReleaseCreate,
    ) -> Result<ProductRelease, StoreError> {
        // §8: the parent product must exist.
        if self.get_product_impl(c.product_id).await?.is_none() {
            return Err(not_found("product", c.product_id));
        }
        let sql = format!(
            r#"INSERT INTO dp_product_releases
                   (org_id, product_id, kind, major, minor, release_notes, released_at, links, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING {RELEASE_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(c.org_id)
            .bind(c.product_id)
            .bind(c.kind.as_str())
            .bind(c.major)
            .bind(c.minor)
            .bind(c.release_notes.as_deref())
            .bind(c.released_at)
            .bind(links_json(&c.links))
            .bind(c.created_by)
            .fetch_one(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row_to_product_release(&row)
    }

    pub(super) async fn update_product_release_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &ProductReleaseUpdate,
    ) -> Result<ProductRelease, StoreError> {
        let sql = format!(
            r#"UPDATE dp_product_releases
                  SET kind=$3, major=$4, minor=$5, release_notes=$6, released_at=$7, links=$8,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {RELEASE_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .bind(u.kind.as_str())
            .bind(u.major)
            .bind(u.minor)
            .bind(u.release_notes.as_deref())
            .bind(u.released_at)
            .bind(links_json(&u.links))
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_product_release(&r),
            None => {
                let exists = self.get_product_release_impl(id).await?.is_some();
                if exists {
                    Err(StoreError::Conflict(format!(
                        "stale version for product release {id}"
                    )))
                } else {
                    Err(not_found("product_release", id))
                }
            }
        }
    }

    pub(super) async fn archive_product_release_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<ProductRelease, StoreError> {
        let current = self
            .get_product_release_impl(id)
            .await?
            .ok_or_else(|| not_found("product_release", id))?;
        if current.archived_at.is_some() {
            return Ok(current);
        }
        let sql = format!(
            r#"UPDATE dp_product_releases
                  SET archived_at = now(), version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {RELEASE_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_product_release(&r),
            None => Err(StoreError::Conflict(format!(
                "stale version for product release {id}"
            ))),
        }
    }
}
