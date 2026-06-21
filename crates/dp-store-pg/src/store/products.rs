//! Store impls for products, product↔project links, and product
//! documents (DOCS/ideas/product-manufacturing.md §5.2).
//!
//! Products follow the §8.2 CAS contract; archive sets `archived_at`
//! AND `status='archived'` so the status column and the soft-delete
//! marker never disagree (the partial-unique model-number index keys
//! on `archived_at IS NULL`). Documents mirror the exec-summary blob
//! pattern verbatim (opaque `blob_ref` jsonb + metadata).

use dp_domain::product::{Product, ProductListFilter, ProductProjectLink, ProductUpsert};
use dp_domain::product_doc::{BlobRefJson, ProductDocument};
use dp_domain::project::Project;
use dp_domain::store::StoreError;
use sqlx::Row;
use uuid::Uuid;

use super::rows::{
    row_to_product, row_to_product_document, row_to_product_project_link, row_to_project,
};
use super::{map_sqlx, not_found, PgStore};

const PRODUCT_COLS: &str = "id, org_id, name, model_number, description, manufacturer_id, \
    status, kind, serial_prefix, serial_format, archived_at, created_by, created_at, \
    updated_at, version";

impl PgStore {
    pub(super) async fn list_products_impl(
        &self,
        filter: &ProductListFilter,
    ) -> Result<Vec<Product>, StoreError> {
        let q = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let sql = format!(
            r#"SELECT {PRODUCT_COLS}
                 FROM dp_products
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%'
                       OR model_number ILIKE '%' || $3 || '%')
             ORDER BY
                  CASE status WHEN 'active' THEN 0 WHEN 'draft' THEN 1
                              WHEN 'eol' THEN 2 ELSE 3 END,
                  lower(name)
                LIMIT $4 OFFSET $5"#
        );
        let rows = sqlx::query(&sql)
            .bind(filter.org_id)
            .bind(status_text)
            .bind(q)
            .bind(filter.limit)
            .bind(filter.offset)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_product).collect()
    }

    pub(super) async fn count_products_impl(
        &self,
        filter: &ProductListFilter,
    ) -> Result<i64, StoreError> {
        let q = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let row = sqlx::query(
            r#"SELECT COUNT(*) AS n FROM dp_products
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%'
                       OR model_number ILIKE '%' || $3 || '%')"#,
        )
        .bind(filter.org_id)
        .bind(status_text)
        .bind(q)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.try_get("n").map_err(map_sqlx)
    }

    pub(super) async fn get_product_impl(&self, id: Uuid) -> Result<Option<Product>, StoreError> {
        let sql = format!("SELECT {PRODUCT_COLS} FROM dp_products WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row.as_ref().map(row_to_product).transpose()
    }

    pub(super) async fn create_product_impl(
        &self,
        u: &ProductUpsert,
    ) -> Result<Product, StoreError> {
        let sql = format!(
            r#"INSERT INTO dp_products
                   (org_id, name, model_number, description, manufacturer_id, status, kind,
                    serial_prefix, serial_format, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
               RETURNING {PRODUCT_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(u.org_id)
            .bind(&u.name)
            .bind(&u.model_number)
            .bind(u.description.as_deref())
            .bind(u.manufacturer_id)
            .bind(u.status.as_str())
            .bind(u.kind.as_str())
            .bind(u.serial_prefix.as_deref())
            .bind(u.serial_format.as_deref())
            .bind(u.created_by)
            .fetch_one(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row_to_product(&row)
    }

    pub(super) async fn update_product_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &ProductUpsert,
    ) -> Result<Product, StoreError> {
        let sql = format!(
            r#"UPDATE dp_products
                  SET name=$3, model_number=$4, description=$5, manufacturer_id=$6,
                      status=$7, serial_prefix=$8, serial_format=$9, kind=$10,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {PRODUCT_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .bind(&u.name)
            .bind(&u.model_number)
            .bind(u.description.as_deref())
            .bind(u.manufacturer_id)
            .bind(u.status.as_str())
            .bind(u.serial_prefix.as_deref())
            .bind(u.serial_format.as_deref())
            .bind(u.kind.as_str())
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_product(&r),
            None => {
                let exists = self.get_product_impl(id).await?.is_some();
                if exists {
                    Err(StoreError::Conflict(format!("stale version for product {id}")))
                } else {
                    Err(not_found("product", id))
                }
            }
        }
    }

    pub(super) async fn archive_product_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Product, StoreError> {
        let current = self
            .get_product_impl(id)
            .await?
            .ok_or_else(|| not_found("product", id))?;
        if current.archived_at.is_some() {
            return Ok(current);
        }
        let sql = format!(
            r#"UPDATE dp_products
                  SET archived_at = now(), status = 'archived',
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {PRODUCT_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_product(&r),
            None => Err(StoreError::Conflict(format!("stale version for product {id}"))),
        }
    }

    // ---- product ↔ project links ----------------------------------

    pub(super) async fn link_product_project_impl(
        &self,
        product_id: Uuid,
        project_id: Uuid,
        linked_by: Option<Uuid>,
    ) -> Result<ProductProjectLink, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_product_project_links (product_id, project_id, linked_by)
               VALUES ($1,$2,$3)
               ON CONFLICT (product_id, project_id)
                 DO UPDATE SET linked_by = COALESCE(EXCLUDED.linked_by,
                                                    dp_product_project_links.linked_by)
               RETURNING id, product_id, project_id, linked_by, linked_at"#,
        )
        .bind(product_id)
        .bind(project_id)
        .bind(linked_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_product_project_link(&row)
    }

    pub(super) async fn unlink_product_project_impl(
        &self,
        product_id: Uuid,
        project_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"DELETE FROM dp_product_project_links
                WHERE product_id = $1 AND project_id = $2"#,
        )
        .bind(product_id)
        .bind(project_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn list_product_projects_impl(
        &self,
        product_id: Uuid,
    ) -> Result<Vec<Project>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT p.*
                 FROM dp_projects p
                 JOIN dp_product_project_links l ON l.project_id = p.id
                WHERE l.product_id = $1
             ORDER BY l.linked_at DESC"#,
        )
            .bind(product_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_project).collect()
    }

    pub(super) async fn list_project_products_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Product>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT pr.*
                 FROM dp_products pr
                 JOIN dp_product_project_links l ON l.product_id = pr.id
                WHERE l.project_id = $1
             ORDER BY l.linked_at DESC"#,
        )
            .bind(project_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_product).collect()
    }

    // ---- product documents (blob upload) --------------------------

    pub(super) async fn list_product_documents_impl(
        &self,
        product_id: Uuid,
    ) -> Result<Vec<ProductDocument>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, product_id, blob_ref, title, doc_type, notes, uploaded_by, created_at
                 FROM dp_product_documents
                WHERE product_id = $1
             ORDER BY created_at DESC"#,
        )
        .bind(product_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_product_document).collect()
    }

    pub(super) async fn get_product_document_impl(
        &self,
        document_id: Uuid,
    ) -> Result<Option<ProductDocument>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, product_id, blob_ref, title, doc_type, notes, uploaded_by, created_at
                 FROM dp_product_documents WHERE id = $1"#,
        )
        .bind(document_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_product_document).transpose()
    }

    pub(super) async fn insert_product_document_impl(
        &self,
        product_id: Uuid,
        blob_ref: &BlobRefJson,
        title: &str,
        doc_type: Option<&str>,
        notes: Option<&str>,
        uploaded_by: Option<&str>,
    ) -> Result<ProductDocument, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_product_documents
                   (product_id, blob_ref, title, doc_type, notes, uploaded_by)
               VALUES ($1,$2,$3,$4,$5,$6)
               RETURNING id, product_id, blob_ref, title, doc_type, notes, uploaded_by, created_at"#,
        )
        .bind(product_id)
        .bind(blob_ref)
        .bind(title)
        .bind(doc_type)
        .bind(notes)
        .bind(uploaded_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_product_document(&row)
    }

    pub(super) async fn delete_product_document_impl(
        &self,
        document_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM dp_product_documents WHERE id = $1")
            .bind(document_id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }
}
