//! Store impls for master-data parties — manufacturers, suppliers,
//! customers (DOCS/ideas/product-manufacturing.md §5.1).
//!
//! Three near-identical CRUD surfaces. Each follows the §8.2 CAS
//! contract on update/archive and the partial-unique name index (a
//! duplicate active name surfaces as [`StoreError::Conflict`] via
//! `map_sqlx`). Archive is idempotent (already-archived → returned
//! unchanged, no version bump), mirroring `archive_project_impl`.

use dp_domain::party::{
    Customer, CustomerUpsert, Manufacturer, ManufacturerUpsert, PartyListFilter, Supplier,
    SupplierUpsert,
};
use dp_domain::store::StoreError;
use sqlx::Row;
use uuid::Uuid;

use super::rows::{row_to_customer, row_to_manufacturer, row_to_supplier};
use super::{map_sqlx, not_found, PgStore};

/// Normalise a free-text search term: trim, drop if empty.
fn norm_q(q: &Option<String>) -> Option<String> {
    q.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

impl PgStore {
    // ===== manufacturers ==========================================

    pub(super) async fn list_manufacturers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<Vec<Manufacturer>, StoreError> {
        let q = norm_q(&filter.q);
        let rows = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, archived_at, created_by, created_at, updated_at, version
                 FROM dp_manufacturers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)
             ORDER BY lower(name)
                LIMIT $4 OFFSET $5"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_manufacturer).collect()
    }

    pub(super) async fn count_manufacturers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<i64, StoreError> {
        let q = norm_q(&filter.q);
        let row = sqlx::query(
            r#"SELECT COUNT(*) AS n FROM dp_manufacturers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.try_get("n").map_err(map_sqlx)
    }

    pub(super) async fn get_manufacturer_impl(
        &self,
        id: Uuid,
    ) -> Result<Option<Manufacturer>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, archived_at, created_by, created_at, updated_at, version
                 FROM dp_manufacturers WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_manufacturer).transpose()
    }

    pub(super) async fn create_manufacturer_impl(
        &self,
        u: &ManufacturerUpsert,
    ) -> Result<Manufacturer, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_manufacturers
                   (org_id, name, contact_name, email, phone, address, website, notes, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(u.org_id)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .bind(u.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_manufacturer(&row)
    }

    pub(super) async fn update_manufacturer_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &ManufacturerUpsert,
    ) -> Result<Manufacturer, StoreError> {
        let row = sqlx::query(
            r#"UPDATE dp_manufacturers
                  SET name=$3, contact_name=$4, email=$5, phone=$6, address=$7,
                      website=$8, notes=$9, version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_manufacturer(&r),
            None => Err(self.party_miss("manufacturer", id, self.get_manufacturer_impl(id).await?.is_some())),
        }
    }

    pub(super) async fn archive_manufacturer_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Manufacturer, StoreError> {
        let current = self
            .get_manufacturer_impl(id)
            .await?
            .ok_or_else(|| not_found("manufacturer", id))?;
        if current.archived_at.is_some() {
            return Ok(current);
        }
        let row = sqlx::query(
            r#"UPDATE dp_manufacturers
                  SET archived_at = now(), version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_manufacturer(&r),
            None => Err(StoreError::Conflict(format!("stale version for manufacturer {id}"))),
        }
    }

    // ===== suppliers ==============================================

    pub(super) async fn list_suppliers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<Vec<Supplier>, StoreError> {
        let q = norm_q(&filter.q);
        let rows = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, archived_at, created_by, created_at, updated_at, version
                 FROM dp_suppliers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)
             ORDER BY lower(name)
                LIMIT $4 OFFSET $5"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_supplier).collect()
    }

    pub(super) async fn count_suppliers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<i64, StoreError> {
        let q = norm_q(&filter.q);
        let row = sqlx::query(
            r#"SELECT COUNT(*) AS n FROM dp_suppliers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.try_get("n").map_err(map_sqlx)
    }

    pub(super) async fn get_supplier_impl(
        &self,
        id: Uuid,
    ) -> Result<Option<Supplier>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, archived_at, created_by, created_at, updated_at, version
                 FROM dp_suppliers WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_supplier).transpose()
    }

    pub(super) async fn create_supplier_impl(
        &self,
        u: &SupplierUpsert,
    ) -> Result<Supplier, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_suppliers
                   (org_id, name, contact_name, email, phone, address, website, notes, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(u.org_id)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .bind(u.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_supplier(&row)
    }

    pub(super) async fn update_supplier_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &SupplierUpsert,
    ) -> Result<Supplier, StoreError> {
        let row = sqlx::query(
            r#"UPDATE dp_suppliers
                  SET name=$3, contact_name=$4, email=$5, phone=$6, address=$7,
                      website=$8, notes=$9, version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_supplier(&r),
            None => Err(self.party_miss("supplier", id, self.get_supplier_impl(id).await?.is_some())),
        }
    }

    pub(super) async fn archive_supplier_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Supplier, StoreError> {
        let current = self
            .get_supplier_impl(id)
            .await?
            .ok_or_else(|| not_found("supplier", id))?;
        if current.archived_at.is_some() {
            return Ok(current);
        }
        let row = sqlx::query(
            r#"UPDATE dp_suppliers
                  SET archived_at = now(), version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_supplier(&r),
            None => Err(StoreError::Conflict(format!("stale version for supplier {id}"))),
        }
    }

    // ===== customers ==============================================

    pub(super) async fn list_customers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<Vec<Customer>, StoreError> {
        let q = norm_q(&filter.q);
        let rows = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, account_ref, archived_at, created_by, created_at, updated_at, version
                 FROM dp_customers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)
             ORDER BY lower(name)
                LIMIT $4 OFFSET $5"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_customer).collect()
    }

    pub(super) async fn count_customers_impl(
        &self,
        filter: &PartyListFilter,
    ) -> Result<i64, StoreError> {
        let q = norm_q(&filter.q);
        let row = sqlx::query(
            r#"SELECT COUNT(*) AS n FROM dp_customers
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR name ILIKE '%' || $2 || '%')
                  AND ($3::bool OR archived_at IS NULL)"#,
        )
        .bind(filter.org_id)
        .bind(q)
        .bind(filter.include_archived)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.try_get("n").map_err(map_sqlx)
    }

    pub(super) async fn get_customer_impl(
        &self,
        id: Uuid,
    ) -> Result<Option<Customer>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, org_id, name, contact_name, email, phone, address, website,
                      notes, account_ref, archived_at, created_by, created_at, updated_at, version
                 FROM dp_customers WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_customer).transpose()
    }

    pub(super) async fn create_customer_impl(
        &self,
        u: &CustomerUpsert,
    ) -> Result<Customer, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_customers
                   (org_id, name, contact_name, email, phone, address, website, notes,
                    account_ref, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, account_ref, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(u.org_id)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .bind(u.account_ref.as_deref())
        .bind(u.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_customer(&row)
    }

    pub(super) async fn update_customer_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &CustomerUpsert,
    ) -> Result<Customer, StoreError> {
        let row = sqlx::query(
            r#"UPDATE dp_customers
                  SET name=$3, contact_name=$4, email=$5, phone=$6, address=$7,
                      website=$8, notes=$9, account_ref=$10,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, account_ref, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .bind(&u.name)
        .bind(u.contact_name.as_deref())
        .bind(u.email.as_deref())
        .bind(u.phone.as_deref())
        .bind(u.address.as_deref())
        .bind(u.website.as_deref())
        .bind(u.notes.as_deref())
        .bind(u.account_ref.as_deref())
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_customer(&r),
            None => Err(self.party_miss("customer", id, self.get_customer_impl(id).await?.is_some())),
        }
    }

    pub(super) async fn archive_customer_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Customer, StoreError> {
        let current = self
            .get_customer_impl(id)
            .await?
            .ok_or_else(|| not_found("customer", id))?;
        if current.archived_at.is_some() {
            return Ok(current);
        }
        let row = sqlx::query(
            r#"UPDATE dp_customers
                  SET archived_at = now(), version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING id, org_id, name, contact_name, email, phone, address, website,
                         notes, account_ref, archived_at, created_by, created_at, updated_at, version"#,
        )
        .bind(id)
        .bind(expected_version)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_customer(&r),
            None => Err(StoreError::Conflict(format!("stale version for customer {id}"))),
        }
    }

    /// Disambiguate a CAS miss: row exists → stale version (Conflict),
    /// row gone → NotFound.
    fn party_miss(&self, entity: &'static str, id: Uuid, exists: bool) -> StoreError {
        if exists {
            StoreError::Conflict(format!("stale version for {entity} {id}"))
        } else {
            not_found(entity, id)
        }
    }
}
