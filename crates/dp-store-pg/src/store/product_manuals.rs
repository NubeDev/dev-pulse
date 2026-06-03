//! Store impls for product manuals + revisions
//! (DOCS/ideas/product-manufacturing.md §5.3).
//!
//! A manual is a CAS-mutable container; revisions are append-only.
//! Publishing is a transaction: the prior published revision (if any)
//! is flipped to `superseded` and the target to `published`, so the
//! partial-unique `one_published` index is never transiently violated.

use dp_domain::product_manual::{
    ManualRevision, ManualUpsert, ProductManual, RevisionUpsert,
};
use dp_domain::store::StoreError;
use uuid::Uuid;

use super::rows::{row_to_manual_revision, row_to_product_manual};
use super::{map_sqlx, not_found, PgStore};

impl PgStore {
    pub(super) async fn list_product_manuals_impl(
        &self,
        product_id: Uuid,
    ) -> Result<Vec<ProductManual>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, product_id, title, created_by, created_at, updated_at, version
                 FROM dp_product_manuals
                WHERE product_id = $1
             ORDER BY created_at DESC"#,
        )
        .bind(product_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_product_manual).collect()
    }

    pub(super) async fn get_product_manual_impl(
        &self,
        manual_id: Uuid,
    ) -> Result<Option<ProductManual>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, product_id, title, created_by, created_at, updated_at, version
                 FROM dp_product_manuals WHERE id = $1"#,
        )
        .bind(manual_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_product_manual).transpose()
    }

    pub(super) async fn create_product_manual_impl(
        &self,
        u: &ManualUpsert,
    ) -> Result<ProductManual, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_product_manuals (product_id, title, created_by)
               VALUES ($1,$2,$3)
               RETURNING id, product_id, title, created_by, created_at, updated_at, version"#,
        )
        .bind(u.product_id)
        .bind(&u.title)
        .bind(u.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_product_manual(&row)
    }

    pub(super) async fn list_manual_revisions_impl(
        &self,
        manual_id: Uuid,
    ) -> Result<Vec<ManualRevision>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, manual_id, revision, status, body_md, change_note,
                      authored_by, created_at
                 FROM dp_product_manual_revisions
                WHERE manual_id = $1
             ORDER BY created_at DESC"#,
        )
        .bind(manual_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_manual_revision).collect()
    }

    pub(super) async fn get_manual_revision_impl(
        &self,
        revision_id: Uuid,
    ) -> Result<Option<ManualRevision>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, manual_id, revision, status, body_md, change_note,
                      authored_by, created_at
                 FROM dp_product_manual_revisions WHERE id = $1"#,
        )
        .bind(revision_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_manual_revision).transpose()
    }

    pub(super) async fn create_manual_revision_impl(
        &self,
        manual_id: Uuid,
        u: &RevisionUpsert,
    ) -> Result<ManualRevision, StoreError> {
        // New revisions are always 'draft'; publishing is a separate op.
        let row = sqlx::query(
            r#"INSERT INTO dp_product_manual_revisions
                   (manual_id, revision, status, body_md, change_note, authored_by)
               VALUES ($1,$2,'draft',$3,$4,$5)
               RETURNING id, manual_id, revision, status, body_md, change_note,
                         authored_by, created_at"#,
        )
        .bind(manual_id)
        .bind(&u.revision)
        .bind(&u.body_md)
        .bind(u.change_note.as_deref())
        .bind(u.authored_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        // Bump the manual's updated_at/version so the container reflects
        // the new revision.
        sqlx::query(
            "UPDATE dp_product_manuals SET version = version + 1, updated_at = now() WHERE id = $1",
        )
        .bind(manual_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_manual_revision(&row)
    }

    pub(super) async fn publish_manual_revision_impl(
        &self,
        manual_id: Uuid,
        revision_id: Uuid,
    ) -> Result<ManualRevision, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Supersede the current published revision (if any, and not the
        // target itself).
        sqlx::query(
            r#"UPDATE dp_product_manual_revisions
                  SET status = 'superseded'
                WHERE manual_id = $1 AND status = 'published' AND id <> $2"#,
        )
        .bind(manual_id)
        .bind(revision_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        // Publish the target.
        let row = sqlx::query(
            r#"UPDATE dp_product_manual_revisions
                  SET status = 'published'
                WHERE id = $1 AND manual_id = $2
               RETURNING id, manual_id, revision, status, body_md, change_note,
                         authored_by, created_at"#,
        )
        .bind(revision_id)
        .bind(manual_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some(row) = row else {
            return Err(not_found("manual_revision", revision_id));
        };
        sqlx::query(
            "UPDATE dp_product_manuals SET version = version + 1, updated_at = now() WHERE id = $1",
        )
        .bind(manual_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        row_to_manual_revision(&row)
    }

    pub(super) async fn list_published_manuals_for_product_impl(
        &self,
        product_id: Uuid,
    ) -> Result<Vec<(ProductManual, ManualRevision)>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT m.id AS m_id, m.product_id AS m_product_id, m.title AS m_title,
                      m.created_by AS m_created_by, m.created_at AS m_created_at,
                      m.updated_at AS m_updated_at, m.version AS m_version,
                      r.id, r.manual_id, r.revision, r.status, r.body_md, r.change_note,
                      r.authored_by, r.created_at
                 FROM dp_product_manuals m
                 JOIN dp_product_manual_revisions r
                   ON r.manual_id = m.id AND r.status = 'published'
                WHERE m.product_id = $1
             ORDER BY m.title"#,
        )
        .bind(product_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        use sqlx::Row;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let manual = ProductManual {
                id: r.try_get("m_id").map_err(map_sqlx)?,
                product_id: r.try_get("m_product_id").map_err(map_sqlx)?,
                title: r.try_get("m_title").map_err(map_sqlx)?,
                created_by: r.try_get("m_created_by").map_err(map_sqlx)?,
                created_at: r.try_get("m_created_at").map_err(map_sqlx)?,
                updated_at: r.try_get("m_updated_at").map_err(map_sqlx)?,
                version: r.try_get("m_version").map_err(map_sqlx)?,
            };
            let revision = row_to_manual_revision(r)?;
            out.push((manual, revision));
        }
        Ok(out)
    }
}
