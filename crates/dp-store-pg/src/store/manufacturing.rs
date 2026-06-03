//! Store impls for manufacturing runs, serialised units, EOL test
//! reports, and the run EOL sign-off summary
//! (DOCS/ideas/product-manufacturing.md §5.4 / §6 + LOCKED DECISION #3).
//!
//! Two behaviours worth knowing before changing them:
//!
//! * **Serial allocation (§6)** reserves a contiguous block of
//!   sequence numbers in ONE atomic UPDATE on `next_serial_seq`
//!   (combined with the `qty_built` bump) and deliberately leaves the
//!   run's user-facing `version` CAS counter untouched — adding units
//!   must not 409 a concurrent status/notes edit, and bulk allocations
//!   must not become CAS retry storms.
//! * **Run counters (§5.4)** are current-state and re-test-safe:
//!   `qty_passed`/`qty_failed` count units by their LATEST EOL outcome.
//!   `record_eol_report` adjusts them only on a *transition* of the
//!   unit's latest outcome (old bucket −1, new bucket +1), never per
//!   insert, so a fail→pass re-test moves buckets instead of
//!   double-counting.

use dp_domain::eol::{EolTestReport, EolTestUpsert, RunEolSummary, RunEolSummaryUpsert};
use dp_domain::manufacturing::{
    ManufacturingRun, ProductUnit, RunUpsert, UnitAllocation, UnitUpsert, MAX_UNIT_ALLOC,
};
use dp_domain::store::StoreError;
use sqlx::Row;
use uuid::Uuid;

use super::rows::{row_to_eol_report, row_to_run, row_to_run_eol_summary, row_to_unit};
use super::{map_sqlx, not_found, PgStore};

const RUN_COLS: &str = "id, org_id, product_id, manufacturer_id, run_code, status, \
    qty_planned, qty_built, qty_passed, qty_failed, next_serial_seq, started_at, \
    completed_at, notes, created_by, created_at, updated_at, version";

const UNIT_COLS: &str = "id, org_id, product_id, run_id, serial_number, status, \
    customer_id, built_at, shipped_at, created_at, updated_at, version";

/// Render a serial from a template (§6). Recognised tokens:
/// `{prefix}`, `{run_code}`, `{seq}` (no pad), `{seq:NN}` (zero-pad to
/// NN). Unknown tokens are left verbatim — the REST layer validates
/// the template on save (`validate_serial_format`).
fn render_serial(fmt: &str, prefix: &str, run_code: &str, seq: i32) -> String {
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut rest = fmt;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end_rel) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let end = start + end_rel;
        let token = &rest[start + 1..end];
        match token {
            "prefix" => out.push_str(prefix),
            "run_code" => out.push_str(run_code),
            "seq" => out.push_str(&seq.to_string()),
            t if t.starts_with("seq:") => {
                let width: usize = t[4..].parse().unwrap_or(0);
                out.push_str(&format!("{seq:0width$}"));
            }
            other => {
                out.push('{');
                out.push_str(other);
                out.push('}');
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

impl PgStore {
    // ---- runs -----------------------------------------------------

    pub(super) async fn list_runs_impl(
        &self,
        product_id: Uuid,
    ) -> Result<Vec<ManufacturingRun>, StoreError> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM dp_manufacturing_runs WHERE product_id = $1 ORDER BY created_at DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(product_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_run).collect()
    }

    pub(super) async fn get_run_impl(&self, id: Uuid) -> Result<Option<ManufacturingRun>, StoreError> {
        let sql = format!("SELECT {RUN_COLS} FROM dp_manufacturing_runs WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row.as_ref().map(row_to_run).transpose()
    }

    pub(super) async fn create_run_impl(&self, u: &RunUpsert) -> Result<ManufacturingRun, StoreError> {
        let sql = format!(
            r#"INSERT INTO dp_manufacturing_runs
                   (org_id, product_id, manufacturer_id, run_code, status, qty_planned,
                    started_at, completed_at, notes, created_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
               RETURNING {RUN_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(u.org_id)
            .bind(u.product_id)
            .bind(u.manufacturer_id)
            .bind(&u.run_code)
            .bind(u.status.as_str())
            .bind(u.qty_planned)
            .bind(u.started_at)
            .bind(u.completed_at)
            .bind(u.notes.as_deref())
            .bind(u.created_by)
            .fetch_one(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row_to_run(&row)
    }

    pub(super) async fn update_run_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &RunUpsert,
    ) -> Result<ManufacturingRun, StoreError> {
        // Status / planned-qty / timing / notes only. Counters and
        // next_serial_seq are maintained by allocation / EOL paths.
        let sql = format!(
            r#"UPDATE dp_manufacturing_runs
                  SET manufacturer_id=$3, run_code=$4, status=$5, qty_planned=$6,
                      started_at=$7, completed_at=$8, notes=$9,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {RUN_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .bind(u.manufacturer_id)
            .bind(&u.run_code)
            .bind(u.status.as_str())
            .bind(u.qty_planned)
            .bind(u.started_at)
            .bind(u.completed_at)
            .bind(u.notes.as_deref())
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_run(&r),
            None => {
                if self.get_run_impl(id).await?.is_some() {
                    Err(StoreError::Conflict(format!("stale version for run {id}")))
                } else {
                    Err(not_found("run", id))
                }
            }
        }
    }

    // ---- units ----------------------------------------------------

    pub(super) async fn list_run_units_impl(
        &self,
        run_id: Uuid,
    ) -> Result<Vec<ProductUnit>, StoreError> {
        let sql = format!(
            "SELECT {UNIT_COLS} FROM dp_product_units WHERE run_id = $1 ORDER BY serial_number"
        );
        let rows = sqlx::query(&sql)
            .bind(run_id)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_unit).collect()
    }

    pub(super) async fn get_unit_impl(&self, id: Uuid) -> Result<Option<ProductUnit>, StoreError> {
        let sql = format!("SELECT {UNIT_COLS} FROM dp_product_units WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        row.as_ref().map(row_to_unit).transpose()
    }

    pub(super) async fn allocate_units_impl(
        &self,
        run_id: Uuid,
        count: i32,
    ) -> Result<UnitAllocation, StoreError> {
        if count <= 0 {
            return Err(StoreError::Invalid("allocation count must be >= 1".into()));
        }
        if count > MAX_UNIT_ALLOC {
            return Err(StoreError::Invalid(format!(
                "allocation count {count} exceeds cap {MAX_UNIT_ALLOC}; chunk the request"
            )));
        }
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Load run + product context (run_code, org/product ids, serial config).
        let ctx = sqlx::query(
            r#"SELECT r.org_id, r.product_id, r.run_code,
                      COALESCE(p.serial_prefix, '') AS serial_prefix,
                      COALESCE(p.serial_format, '{prefix}-{run_code}-{seq:05}') AS serial_format
                 FROM dp_manufacturing_runs r
                 JOIN dp_products p ON p.id = r.product_id
                WHERE r.id = $1
                FOR UPDATE OF r"#,
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some(ctx) = ctx else {
            return Err(not_found("run", run_id));
        };
        let org_id: Uuid = ctx.try_get("org_id").map_err(map_sqlx)?;
        let product_id: Uuid = ctx.try_get("product_id").map_err(map_sqlx)?;
        let run_code: String = ctx.try_get("run_code").map_err(map_sqlx)?;
        let prefix: String = ctx.try_get("serial_prefix").map_err(map_sqlx)?;
        let fmt: String = ctx.try_get("serial_format").map_err(map_sqlx)?;

        // Atomic reservation (§6): bump next_serial_seq + qty_built in one
        // statement; do NOT touch `version`.
        let reserved = sqlx::query(
            r#"UPDATE dp_manufacturing_runs
                  SET next_serial_seq = next_serial_seq + $2,
                      qty_built       = qty_built + $2,
                      updated_at      = now()
                WHERE id = $1
               RETURNING next_serial_seq - $2 AS first_seq"#,
        )
        .bind(run_id)
        .bind(count)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let first_seq: i32 = reserved.try_get("first_seq").map_err(map_sqlx)?;

        // Format the serial block and bulk insert via UNNEST.
        let serials: Vec<String> = (0..count)
            .map(|i| render_serial(&fmt, &prefix, &run_code, first_seq + i))
            .collect();
        let insert_sql = format!(
            r#"INSERT INTO dp_product_units
                   (org_id, product_id, run_id, serial_number, status, built_at)
               SELECT $1, $2, $3, s, 'built', now()
                 FROM unnest($4::text[]) AS s
               RETURNING {UNIT_COLS}"#
        );
        let rows = sqlx::query(&insert_sql)
            .bind(org_id)
            .bind(product_id)
            .bind(run_id)
            .bind(&serials)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        let mut units: Vec<ProductUnit> = rows.iter().map(row_to_unit).collect::<Result<_, _>>()?;
        units.sort_by(|a, b| a.serial_number.cmp(&b.serial_number));

        tx.commit().await.map_err(map_sqlx)?;
        Ok(UnitAllocation { units, first_seq, count })
    }

    pub(super) async fn update_unit_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        u: &UnitUpsert,
    ) -> Result<ProductUnit, StoreError> {
        let sql = format!(
            r#"UPDATE dp_product_units
                  SET status=$3, customer_id=$4, built_at=$5, shipped_at=$6,
                      version = version + 1, updated_at = now()
                WHERE id=$1 AND version=$2
               RETURNING {UNIT_COLS}"#
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(expected_version)
            .bind(u.status.as_str())
            .bind(u.customer_id)
            .bind(u.built_at)
            .bind(u.shipped_at)
            .fetch_optional(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_unit(&r),
            None => {
                if self.get_unit_impl(id).await?.is_some() {
                    Err(StoreError::Conflict(format!("stale version for unit {id}")))
                } else {
                    Err(not_found("unit", id))
                }
            }
        }
    }

    // ---- EOL reports + re-test-safe run counters (§5.4) -----------

    pub(super) async fn list_unit_eol_reports_impl(
        &self,
        unit_id: Uuid,
    ) -> Result<Vec<EolTestReport>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, unit_id, result, station, firmware, measurements, log_blob_ref,
                      notes, tested_by, tested_at, created_at
                 FROM dp_eol_test_reports
                WHERE unit_id = $1
             ORDER BY tested_at DESC, created_at DESC"#,
        )
        .bind(unit_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_eol_report).collect()
    }

    pub(super) async fn record_eol_report_impl(
        &self,
        unit_id: Uuid,
        u: &EolTestUpsert,
    ) -> Result<EolTestReport, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        // Lock the unit row; get its run.
        let unit_row = sqlx::query(
            "SELECT run_id, status FROM dp_product_units WHERE id = $1 FOR UPDATE",
        )
        .bind(unit_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some(unit_row) = unit_row else {
            return Err(not_found("unit", unit_id));
        };
        let run_id: Option<Uuid> = unit_row.try_get("run_id").map_err(map_sqlx)?;

        // The unit's latest outcome BEFORE this insert (None if untested).
        let prior: Option<String> = sqlx::query_scalar(
            r#"SELECT result FROM dp_eol_test_reports
                WHERE unit_id = $1
             ORDER BY tested_at DESC, created_at DESC
                LIMIT 1"#,
        )
        .bind(unit_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        // Insert the new report — it becomes the latest (tested_at now()).
        let measurements = if u.measurements.is_null() {
            serde_json::json!({})
        } else {
            u.measurements.clone()
        };
        let row = sqlx::query(
            r#"INSERT INTO dp_eol_test_reports
                   (unit_id, result, station, firmware, measurements, log_blob_ref, notes, tested_by)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
               RETURNING id, unit_id, result, station, firmware, measurements, log_blob_ref,
                         notes, tested_by, tested_at, created_at"#,
        )
        .bind(unit_id)
        .bind(u.result.as_str())
        .bind(u.station.as_deref())
        .bind(u.firmware.as_deref())
        .bind(&measurements)
        .bind(u.log_blob_ref.as_ref())
        .bind(u.notes.as_deref())
        .bind(u.tested_by.as_deref())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        // Re-test-safe counter maintenance (§5.4): adjust only on a
        // transition of the unit's latest outcome.
        if let Some(run_id) = run_id {
            let new_outcome = u.result.as_str();
            let old_outcome = prior.as_deref();
            let (d_pass, d_fail): (i32, i32) = match (old_outcome, new_outcome) {
                (Some("pass"), "pass") | (Some("fail"), "fail") => (0, 0),
                (None, "pass") => (1, 0),
                (None, "fail") => (0, 1),
                (Some("fail"), "pass") => (1, -1),
                (Some("pass"), "fail") => (-1, 1),
                // Defensive: unknown prior value treated as untested.
                (Some(_), "pass") => (1, 0),
                (Some(_), "fail") => (0, 1),
                _ => (0, 0),
            };
            if d_pass != 0 || d_fail != 0 {
                sqlx::query(
                    r#"UPDATE dp_manufacturing_runs
                          SET qty_passed = qty_passed + $2,
                              qty_failed = qty_failed + $3,
                              updated_at = now()
                        WHERE id = $1"#,
                )
                .bind(run_id)
                .bind(d_pass)
                .bind(d_fail)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx)?;
            }
        }

        // Mark the unit tested (built → tested); no version bump — this
        // is an automatic side effect, like serial allocation (§6).
        sqlx::query(
            r#"UPDATE dp_product_units
                  SET status = CASE WHEN status = 'built' THEN 'tested' ELSE status END,
                      updated_at = now()
                WHERE id = $1"#,
        )
        .bind(unit_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        row_to_eol_report(&row)
    }

    // ---- run EOL sign-off summary (LOCKED DECISION #3) ------------

    pub(super) async fn get_run_eol_summary_impl(
        &self,
        run_id: Uuid,
    ) -> Result<Option<RunEolSummary>, StoreError> {
        let row = sqlx::query(
            r#"SELECT run_id, built_count, pass_count, fail_count, notes_md,
                      signed_by, signed_at, created_at, updated_at, version
                 FROM dp_run_eol_summary WHERE run_id = $1"#,
        )
        .bind(run_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_run_eol_summary).transpose()
    }

    pub(super) async fn upsert_run_eol_summary_impl(
        &self,
        run_id: Uuid,
        u: &RunEolSummaryUpsert,
    ) -> Result<RunEolSummary, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Snapshot the run's current counters.
        let counts = sqlx::query(
            "SELECT qty_built, qty_passed, qty_failed FROM dp_manufacturing_runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let Some(counts) = counts else {
            return Err(not_found("run", run_id));
        };
        let built: i32 = counts.try_get("qty_built").map_err(map_sqlx)?;
        let passed: i32 = counts.try_get("qty_passed").map_err(map_sqlx)?;
        let failed: i32 = counts.try_get("qty_failed").map_err(map_sqlx)?;

        let signed_by = if u.sign_off { u.signed_by } else { None };
        let row = sqlx::query(
            r#"INSERT INTO dp_run_eol_summary
                   (run_id, built_count, pass_count, fail_count, notes_md, signed_by, signed_at)
               VALUES ($1,$2,$3,$4,$5,$6, CASE WHEN $7 THEN now() ELSE NULL END)
               ON CONFLICT (run_id) DO UPDATE SET
                   built_count = EXCLUDED.built_count,
                   pass_count  = EXCLUDED.pass_count,
                   fail_count  = EXCLUDED.fail_count,
                   notes_md    = EXCLUDED.notes_md,
                   signed_by   = CASE WHEN $7 THEN EXCLUDED.signed_by ELSE dp_run_eol_summary.signed_by END,
                   signed_at   = CASE WHEN $7 THEN now() ELSE dp_run_eol_summary.signed_at END,
                   version     = dp_run_eol_summary.version + 1,
                   updated_at  = now()
               RETURNING run_id, built_count, pass_count, fail_count, notes_md,
                         signed_by, signed_at, created_at, updated_at, version"#,
        )
        .bind(run_id)
        .bind(built)
        .bind(passed)
        .bind(failed)
        .bind(u.notes_md.as_deref())
        .bind(signed_by)
        .bind(u.sign_off)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;

        tx.commit().await.map_err(map_sqlx)?;
        row_to_run_eol_summary(&row)
    }
}

#[cfg(test)]
mod tests {
    use super::render_serial;

    #[test]
    fn render_serial_default_template() {
        assert_eq!(
            render_serial("{prefix}-{run_code}-{seq:05}", "NB", "R2026-014", 42),
            "NB-R2026-014-00042"
        );
    }

    #[test]
    fn render_serial_plain_seq_and_unknown_token() {
        assert_eq!(render_serial("S{seq}", "", "R1", 7), "S7");
        // Unknown token left verbatim (validator rejects these on save).
        assert_eq!(render_serial("{prefix}{bogus}{seq}", "X", "R1", 1), "X{bogus}1");
    }

    #[test]
    fn render_serial_no_prefix() {
        assert_eq!(render_serial("{run_code}/{seq:03}", "", "B7", 5), "B7/005");
    }
}
