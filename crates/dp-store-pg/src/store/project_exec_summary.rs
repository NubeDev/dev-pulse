//! Postgres-backed bodies for the project executive summary surface.
//!
//! Schema lives in
//! [`0045_project_exec_summary.sql`](../../../migrations/dp/0045_project_exec_summary.sql);
//! the [`Store`] trait declarations live in
//! [`dp_domain::store`](../../../../dp-domain/src/store.rs).
//!
//! Every body here is a thin SQL wrapper. Behaviour notes:
//!
//! * **Lazy materialisation.** [`get_project_exec_summary_impl`]
//!   returns `Ok(None)` when no row exists yet rather than auto-
//!   creating; the REST PATCH path calls
//!   [`upsert_project_exec_summary_impl`] which is `INSERT ... ON
//!   CONFLICT DO NOTHING` + `SELECT` so a concurrent first-edit
//!   race is harmless.
//!
//! * **Sparse PATCH via `COALESCE`.** [`patch_project_exec_summary_impl`]
//!   sends every column on every PATCH, falling back to the existing
//!   value when the caller's [`ProjectExecSummaryPatch`] field is
//!   `None`. To distinguish "absent" from "set to NULL" the body
//!   passes a second `_is_set` boolean per nullable column and the
//!   `CASE WHEN $N THEN $value ELSE col END` clause picks. This is
//!   verbose but keeps the SQL static (no string-stitching) and the
//!   round-trip stays one statement.
//!
//! * **State machine.** Status transitions go through
//!   `submit_*` / `approve_*` / `revert_*`. Each one CAS-gates on
//!   the expected current status and returns
//!   [`StoreError::Conflict`] on a mismatch so the REST handler can
//!   surface a 409.
//!
//! * **Completion is computed in SQL.** The eight-section completion
//!   booleans are projected alongside the row in
//!   [`get_project_exec_summary_impl`] so the GET response stays one
//!   round-trip. Rules mirror §3.5 of the scope doc; see the
//!   `completion_*` CTE columns.

use chrono::NaiveDate;
use dp_domain::project_exec_summary::{
    BlobRefJson, ExecSummaryChangelogEntry, ExecSummaryChangelogInsert, ExecSummaryCompletion,
    ExecSummaryDocument, ExecSummaryImage, ExecSummaryStatus, ProjectExecSummary,
    ProjectExecSummaryPatch,
};
use dp_domain::store::StoreError;
use serde_json::Value as JsonValue;
use sqlx::Row;
use uuid::Uuid;

use super::{invalid, map_sqlx, not_found, PgStore};

impl PgStore {
    // ----- summary row -------------------------------------------------

    pub(super) async fn get_project_exec_summary_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Option<(ProjectExecSummary, ExecSummaryCompletion)>, StoreError> {
        // The completion booleans are derived from the same row plus
        // EXISTS subqueries on the three child tables. Keeping the
        // logic in SQL means the GET handler doesn't have to issue a
        // second round-trip for child-row counts and the rules from
        // scope §3.5 are co-located with the schema.
        let row = sqlx::query(
            r#"
            SELECT s.*,
                   (s.product_name IS NOT NULL AND s.product_name <> ''
                    AND s.objective IS NOT NULL AND s.objective <> ''
                    AND s.success_criteria IS NOT NULL AND s.success_criteria <> '')
                       AS c_summary,
                   (s.in_scope IS NOT NULL AND s.in_scope <> ''
                    AND s.out_of_scope IS NOT NULL AND s.out_of_scope <> '')
                       AS c_scope,
                   (s.must_have IS NOT NULL AND s.must_have <> ''
                    AND COALESCE(array_length(s.protocols, 1), 0) >= 1)
                       AS c_requirements,
                   ((s.hardware_features IS NOT NULL AND s.hardware_features <> '')
                    OR EXISTS (SELECT 1 FROM dp_project_exec_summary_images
                                WHERE project_id = s.project_id))
                       AS c_hardware,
                   (s.rrp_cents IS NOT NULL AND s.target_gp_bp IS NOT NULL)
                       AS c_commercial,
                   EXISTS (SELECT 1 FROM dp_project_exec_summary_documents
                            WHERE project_id = s.project_id)
                       AS c_documents,
                   (s.status = 'approved')
                       AS c_approval,
                   EXISTS (SELECT 1 FROM dp_project_exec_summary_changelog
                            WHERE project_id = s.project_id)
                       AS c_changelog
              FROM dp_project_exec_summary s
             WHERE s.project_id = $1
            "#,
        )
        .bind(project_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let Some(r) = row else { return Ok(None) };
        let summary = row_to_exec_summary(&r)?;
        let completion = ExecSummaryCompletion {
            summary: r.try_get("c_summary").map_err(map_sqlx)?,
            scope: r.try_get("c_scope").map_err(map_sqlx)?,
            requirements: r.try_get("c_requirements").map_err(map_sqlx)?,
            hardware: r.try_get("c_hardware").map_err(map_sqlx)?,
            commercial: r.try_get("c_commercial").map_err(map_sqlx)?,
            documents: r.try_get("c_documents").map_err(map_sqlx)?,
            approval: r.try_get("c_approval").map_err(map_sqlx)?,
            changelog: r.try_get("c_changelog").map_err(map_sqlx)?,
        };
        Ok(Some((summary, completion)))
    }

    pub(super) async fn upsert_project_exec_summary_impl(
        &self,
        project_id: Uuid,
    ) -> Result<ProjectExecSummary, StoreError> {
        // INSERT ... ON CONFLICT DO NOTHING + RETURNING does not
        // return the row on conflict, so we follow with a SELECT.
        // The pair is not atomic across the two statements, but
        // that's fine — both branches converge on "the row exists"
        // and the body is idempotent. The project_id FK guards
        // against creating a summary for a deleted project.
        sqlx::query(
            r#"INSERT INTO dp_project_exec_summary (project_id)
                    VALUES ($1)
               ON CONFLICT (project_id) DO NOTHING"#,
        )
        .bind(project_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        let row = sqlx::query(
            r#"SELECT * FROM dp_project_exec_summary WHERE project_id = $1"#,
        )
        .bind(project_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref()
            .map(row_to_exec_summary)
            .ok_or_else(|| not_found("project_exec_summary", project_id))?
    }

    pub(super) async fn patch_project_exec_summary_impl(
        &self,
        project_id: Uuid,
        patch: &ProjectExecSummaryPatch,
    ) -> Result<ProjectExecSummary, StoreError> {
        // Sparse PATCH: for each nullable column, we bind a `_set`
        // boolean and a candidate value; the SQL picks the candidate
        // when set and the existing column otherwise. Protocols
        // (TEXT[], not nullable) gets a simpler `COALESCE` over a
        // `_set` flag.
        //
        // The `protocols` array binding goes through sqlx's native
        // `Vec<String>` → `TEXT[]` mapping. The candidate is always
        // a non-NULL array (empty when `Some(vec![])`) so the
        // `_set ? candidate : protocols` switch is honest.
        //
        // Order of binds: the candidate then the `_set` for each
        // nullable column, then the protocols pair, then the WHERE
        // id. Keep the ordering in sync with the `$N` placeholders.

        macro_rules! opt_pair {
            ($p:expr) => {
                ($p.as_ref().and_then(|v| v.as_ref()), $p.is_some())
            };
        }

        let (product_name, product_name_set) = opt_pair!(patch.product_name);
        let (part_number, part_number_set) = opt_pair!(patch.part_number);
        let (target_release_date, target_release_date_set): (Option<&NaiveDate>, bool) =
            (patch.target_release_date.as_ref().and_then(|v| v.as_ref()), patch.target_release_date.is_some());
        let (objective, objective_set) = opt_pair!(patch.objective);
        let (problem, problem_set) = opt_pair!(patch.problem);
        let (value, value_set) = opt_pair!(patch.value);
        let (differentiators, differentiators_set) = opt_pair!(patch.differentiators);
        let (success_criteria, success_criteria_set) = opt_pair!(patch.success_criteria);

        let (in_scope, in_scope_set) = opt_pair!(patch.in_scope);
        let (out_of_scope, out_of_scope_set) = opt_pair!(patch.out_of_scope);
        let (assumptions, assumptions_set) = opt_pair!(patch.assumptions);
        let (dependencies, dependencies_set) = opt_pair!(patch.dependencies);
        let (constraints, constraints_set) = opt_pair!(patch.constraints);

        let (must_have, must_have_set) = opt_pair!(patch.must_have);
        let (optional_, optional_set) = opt_pair!(patch.optional);
        let (user_interaction, user_interaction_set) = opt_pair!(patch.user_interaction);
        let (architecture, architecture_set) = opt_pair!(patch.architecture);
        let protocols_set = patch.protocols.is_some();
        let protocols_val: Vec<String> = patch.protocols.clone().unwrap_or_default();
        let (power, power_set) = opt_pair!(patch.power);
        let (mounting, mounting_set) = opt_pair!(patch.mounting);
        let (certification, certification_set) = opt_pair!(patch.certification);

        let (hardware_features, hardware_features_set) = opt_pair!(patch.hardware_features);
        let (physical_notes, physical_notes_set) = opt_pair!(patch.physical_notes);
        let (enclosure, enclosure_set) = opt_pair!(patch.enclosure);
        let (mounting_type, mounting_type_set) = opt_pair!(patch.mounting_type);
        let (operating_env, operating_env_set) = opt_pair!(patch.operating_env);

        let (rrp_cents, rrp_cents_set): (Option<&i64>, bool) =
            (patch.rrp_cents.as_ref().and_then(|v| v.as_ref()), patch.rrp_cents.is_some());
        let (oem_price_cents, oem_price_cents_set): (Option<&i64>, bool) = (
            patch.oem_price_cents.as_ref().and_then(|v| v.as_ref()),
            patch.oem_price_cents.is_some(),
        );
        let (target_gp_bp, target_gp_bp_set): (Option<&i64>, bool) = (
            patch.target_gp_bp.as_ref().and_then(|v| v.as_ref()),
            patch.target_gp_bp.is_some(),
        );
        let (revenue_model, revenue_model_set) = opt_pair!(patch.revenue_model);
        let (channel_strategy, channel_strategy_set) = opt_pair!(patch.channel_strategy);
        let (target_market, target_market_set) = opt_pair!(patch.target_market);
        let (volume_assumptions, volume_assumptions_set) = opt_pair!(patch.volume_assumptions);

        let (reviewer, reviewer_set) = opt_pair!(patch.reviewer);
        let (approver, approver_set) = opt_pair!(patch.approver);
        let (review_notes, review_notes_set) = opt_pair!(patch.review_notes);
        let (approval_notes, approval_notes_set) = opt_pair!(patch.approval_notes);

        let row = sqlx::query(
            r#"
            UPDATE dp_project_exec_summary SET
                product_name        = CASE WHEN $2  THEN $3  ELSE product_name        END,
                part_number         = CASE WHEN $4  THEN $5  ELSE part_number         END,
                target_release_date = CASE WHEN $6  THEN $7  ELSE target_release_date END,
                objective           = CASE WHEN $8  THEN $9  ELSE objective           END,
                problem             = CASE WHEN $10 THEN $11 ELSE problem             END,
                value               = CASE WHEN $12 THEN $13 ELSE value               END,
                differentiators     = CASE WHEN $14 THEN $15 ELSE differentiators     END,
                success_criteria    = CASE WHEN $16 THEN $17 ELSE success_criteria    END,

                in_scope            = CASE WHEN $18 THEN $19 ELSE in_scope            END,
                out_of_scope        = CASE WHEN $20 THEN $21 ELSE out_of_scope        END,
                assumptions         = CASE WHEN $22 THEN $23 ELSE assumptions         END,
                dependencies        = CASE WHEN $24 THEN $25 ELSE dependencies        END,
                constraints         = CASE WHEN $26 THEN $27 ELSE constraints         END,

                must_have           = CASE WHEN $28 THEN $29 ELSE must_have           END,
                optional            = CASE WHEN $30 THEN $31 ELSE optional            END,
                user_interaction    = CASE WHEN $32 THEN $33 ELSE user_interaction    END,
                architecture        = CASE WHEN $34 THEN $35 ELSE architecture        END,
                protocols           = CASE WHEN $36 THEN $37 ELSE protocols           END,
                power               = CASE WHEN $38 THEN $39 ELSE power               END,
                mounting            = CASE WHEN $40 THEN $41 ELSE mounting            END,
                certification       = CASE WHEN $42 THEN $43 ELSE certification       END,

                hardware_features   = CASE WHEN $44 THEN $45 ELSE hardware_features   END,
                physical_notes      = CASE WHEN $46 THEN $47 ELSE physical_notes      END,
                enclosure           = CASE WHEN $48 THEN $49 ELSE enclosure           END,
                mounting_type       = CASE WHEN $50 THEN $51 ELSE mounting_type       END,
                operating_env       = CASE WHEN $52 THEN $53 ELSE operating_env       END,

                rrp_cents           = CASE WHEN $54 THEN $55 ELSE rrp_cents           END,
                oem_price_cents     = CASE WHEN $56 THEN $57 ELSE oem_price_cents     END,
                target_gp_bp        = CASE WHEN $58 THEN $59 ELSE target_gp_bp        END,
                revenue_model       = CASE WHEN $60 THEN $61 ELSE revenue_model       END,
                channel_strategy    = CASE WHEN $62 THEN $63 ELSE channel_strategy    END,
                target_market       = CASE WHEN $64 THEN $65 ELSE target_market       END,
                volume_assumptions  = CASE WHEN $66 THEN $67 ELSE volume_assumptions  END,

                reviewer            = CASE WHEN $68 THEN $69 ELSE reviewer            END,
                approver            = CASE WHEN $70 THEN $71 ELSE approver            END,
                review_notes        = CASE WHEN $72 THEN $73 ELSE review_notes        END,
                approval_notes      = CASE WHEN $74 THEN $75 ELSE approval_notes      END,

                updated_at          = now()
            WHERE project_id = $1
            RETURNING *
            "#,
        )
        .bind(project_id)
        .bind(product_name_set).bind(product_name)
        .bind(part_number_set).bind(part_number)
        .bind(target_release_date_set).bind(target_release_date)
        .bind(objective_set).bind(objective)
        .bind(problem_set).bind(problem)
        .bind(value_set).bind(value)
        .bind(differentiators_set).bind(differentiators)
        .bind(success_criteria_set).bind(success_criteria)
        .bind(in_scope_set).bind(in_scope)
        .bind(out_of_scope_set).bind(out_of_scope)
        .bind(assumptions_set).bind(assumptions)
        .bind(dependencies_set).bind(dependencies)
        .bind(constraints_set).bind(constraints)
        .bind(must_have_set).bind(must_have)
        .bind(optional_set).bind(optional_)
        .bind(user_interaction_set).bind(user_interaction)
        .bind(architecture_set).bind(architecture)
        .bind(protocols_set).bind(&protocols_val)
        .bind(power_set).bind(power)
        .bind(mounting_set).bind(mounting)
        .bind(certification_set).bind(certification)
        .bind(hardware_features_set).bind(hardware_features)
        .bind(physical_notes_set).bind(physical_notes)
        .bind(enclosure_set).bind(enclosure)
        .bind(mounting_type_set).bind(mounting_type)
        .bind(operating_env_set).bind(operating_env)
        .bind(rrp_cents_set).bind(rrp_cents)
        .bind(oem_price_cents_set).bind(oem_price_cents)
        .bind(target_gp_bp_set).bind(target_gp_bp)
        .bind(revenue_model_set).bind(revenue_model)
        .bind(channel_strategy_set).bind(channel_strategy)
        .bind(target_market_set).bind(target_market)
        .bind(volume_assumptions_set).bind(volume_assumptions)
        .bind(reviewer_set).bind(reviewer)
        .bind(approver_set).bind(approver)
        .bind(review_notes_set).bind(review_notes)
        .bind(approval_notes_set).bind(approval_notes)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;

        match row {
            Some(r) => row_to_exec_summary(&r),
            None => Err(not_found("project_exec_summary", project_id)),
        }
    }

    pub(super) async fn submit_project_exec_summary_impl(
        &self,
        project_id: Uuid,
    ) -> Result<ProjectExecSummary, StoreError> {
        self.transition_status_impl(project_id, "draft", "in_review", true, false, None)
            .await
    }

    pub(super) async fn approve_project_exec_summary_impl(
        &self,
        project_id: Uuid,
        approval_notes: Option<&str>,
    ) -> Result<ProjectExecSummary, StoreError> {
        self.transition_status_impl(
            project_id,
            "in_review",
            "approved",
            false,
            true,
            approval_notes,
        )
        .await
    }

    pub(super) async fn revert_project_exec_summary_impl(
        &self,
        project_id: Uuid,
    ) -> Result<ProjectExecSummary, StoreError> {
        // Unconditional: any → draft. We don't enforce a `from`
        // status so a stale UI clicking Revert during a race still
        // succeeds. `submitted_at` / `approved_at` are preserved so
        // history stays visible after revert (scope §3.4 E3).
        let row = sqlx::query(
            r#"UPDATE dp_project_exec_summary
                  SET status     = 'draft',
                      updated_at = now()
                WHERE project_id = $1
            RETURNING *"#,
        )
        .bind(project_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_exec_summary(&r),
            None => Err(not_found("project_exec_summary", project_id)),
        }
    }

    async fn transition_status_impl(
        &self,
        project_id: Uuid,
        from: &'static str,
        to: &'static str,
        touch_submitted_at: bool,
        touch_approved_at: bool,
        approval_notes: Option<&str>,
    ) -> Result<ProjectExecSummary, StoreError> {
        // CAS-style: WHERE project_id = ? AND status = ?. A miss is
        // either "no row" (NotFound) or "wrong status" (Conflict). One
        // extra SELECT disambiguates; the cost is negligible against
        // the human-cadence of a submit / approve click.
        let row = sqlx::query(
            r#"UPDATE dp_project_exec_summary
                  SET status         = $3,
                      submitted_at   = CASE WHEN $4 THEN now() ELSE submitted_at END,
                      approved_at    = CASE WHEN $5 THEN now() ELSE approved_at  END,
                      approval_notes = COALESCE($6, approval_notes),
                      updated_at     = now()
                WHERE project_id = $1 AND status = $2
            RETURNING *"#,
        )
        .bind(project_id)
        .bind(from)
        .bind(to)
        .bind(touch_submitted_at)
        .bind(touch_approved_at)
        .bind(approval_notes)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if let Some(r) = row {
            return row_to_exec_summary(&r);
        }
        // Disambiguate the miss.
        let current = self.get_project_exec_summary_impl(project_id).await?;
        match current {
            None => Err(not_found("project_exec_summary", project_id)),
            Some((s, _)) => Err(StoreError::Conflict(format!(
                "exec summary status is {:?}, expected {from}",
                s.status
            ))),
        }
    }

    // ----- images ------------------------------------------------------

    pub(super) async fn list_exec_summary_images_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ExecSummaryImage>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, blob_ref, filename, content_type, caption, ord, created_at
                 FROM dp_project_exec_summary_images
                WHERE project_id = $1
                ORDER BY ord ASC, created_at ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_exec_summary_image).collect()
    }

    pub(super) async fn get_exec_summary_image_impl(
        &self,
        image_id: Uuid,
    ) -> Result<Option<ExecSummaryImage>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, project_id, blob_ref, filename, content_type, caption, ord, created_at
                 FROM dp_project_exec_summary_images
                WHERE id = $1"#,
        )
        .bind(image_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_exec_summary_image).transpose()
    }

    pub(super) async fn insert_exec_summary_image_impl(
        &self,
        project_id: Uuid,
        blob_ref: &BlobRefJson,
        filename: &str,
        content_type: &str,
        caption: Option<&str>,
        ord: Option<i32>,
    ) -> Result<ExecSummaryImage, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_project_exec_summary_images
                   (project_id, blob_ref, filename, content_type, caption, ord)
               VALUES ($1, $2, $3, $4, $5, COALESCE($6, 0))
            RETURNING id, project_id, blob_ref, filename, content_type, caption, ord, created_at"#,
        )
        .bind(project_id)
        .bind(blob_ref)
        .bind(filename)
        .bind(content_type)
        .bind(caption)
        .bind(ord)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_exec_summary_image(&row)
    }

    pub(super) async fn update_exec_summary_image_impl(
        &self,
        image_id: Uuid,
        caption: Option<Option<String>>,
        ord: Option<i32>,
    ) -> Result<ExecSummaryImage, StoreError> {
        let caption_set = caption.is_some();
        let caption_val: Option<String> = caption.flatten();
        let ord_set = ord.is_some();
        let row = sqlx::query(
            r#"UPDATE dp_project_exec_summary_images SET
                   caption = CASE WHEN $2 THEN $3 ELSE caption END,
                   ord     = CASE WHEN $4 THEN $5 ELSE ord     END
               WHERE id = $1
            RETURNING id, project_id, blob_ref, filename, content_type, caption, ord, created_at"#,
        )
        .bind(image_id)
        .bind(caption_set).bind(caption_val)
        .bind(ord_set).bind(ord)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_exec_summary_image(&r),
            None => Err(not_found("exec_summary_image", image_id)),
        }
    }

    pub(super) async fn delete_exec_summary_image_impl(
        &self,
        image_id: Uuid,
    ) -> Result<(), StoreError> {
        let res = sqlx::query("DELETE FROM dp_project_exec_summary_images WHERE id = $1")
            .bind(image_id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("exec_summary_image", image_id));
        }
        Ok(())
    }

    // ----- documents ---------------------------------------------------

    pub(super) async fn list_exec_summary_documents_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ExecSummaryDocument>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, blob_ref, title, doc_type, notes,
                      required_action, uploaded_by, created_at
                 FROM dp_project_exec_summary_documents
                WHERE project_id = $1
                ORDER BY created_at DESC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_exec_summary_document).collect()
    }

    pub(super) async fn get_exec_summary_document_impl(
        &self,
        document_id: Uuid,
    ) -> Result<Option<ExecSummaryDocument>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, project_id, blob_ref, title, doc_type, notes,
                      required_action, uploaded_by, created_at
                 FROM dp_project_exec_summary_documents
                WHERE id = $1"#,
        )
        .bind(document_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.as_ref().map(row_to_exec_summary_document).transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn insert_exec_summary_document_impl(
        &self,
        project_id: Uuid,
        blob_ref: &BlobRefJson,
        title: &str,
        doc_type: Option<&str>,
        notes: Option<&str>,
        required_action: Option<&str>,
        uploaded_by: Option<&str>,
    ) -> Result<ExecSummaryDocument, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_project_exec_summary_documents
                   (project_id, blob_ref, title, doc_type, notes, required_action, uploaded_by)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, project_id, blob_ref, title, doc_type, notes,
                      required_action, uploaded_by, created_at"#,
        )
        .bind(project_id)
        .bind(blob_ref)
        .bind(title)
        .bind(doc_type)
        .bind(notes)
        .bind(required_action)
        .bind(uploaded_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_exec_summary_document(&row)
    }

    pub(super) async fn update_exec_summary_document_impl(
        &self,
        document_id: Uuid,
        title: Option<String>,
        doc_type: Option<Option<String>>,
        notes: Option<Option<String>>,
        required_action: Option<Option<String>>,
    ) -> Result<ExecSummaryDocument, StoreError> {
        let title_set = title.is_some();
        let doc_type_set = doc_type.is_some();
        let doc_type_val: Option<String> = doc_type.flatten();
        let notes_set = notes.is_some();
        let notes_val: Option<String> = notes.flatten();
        let required_action_set = required_action.is_some();
        let required_action_val: Option<String> = required_action.flatten();

        let row = sqlx::query(
            r#"UPDATE dp_project_exec_summary_documents SET
                   title           = CASE WHEN $2 THEN $3 ELSE title           END,
                   doc_type        = CASE WHEN $4 THEN $5 ELSE doc_type        END,
                   notes           = CASE WHEN $6 THEN $7 ELSE notes           END,
                   required_action = CASE WHEN $8 THEN $9 ELSE required_action END
               WHERE id = $1
            RETURNING id, project_id, blob_ref, title, doc_type, notes,
                      required_action, uploaded_by, created_at"#,
        )
        .bind(document_id)
        .bind(title_set).bind(title)
        .bind(doc_type_set).bind(doc_type_val)
        .bind(notes_set).bind(notes_val)
        .bind(required_action_set).bind(required_action_val)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_exec_summary_document(&r),
            None => Err(not_found("exec_summary_document", document_id)),
        }
    }

    pub(super) async fn delete_exec_summary_document_impl(
        &self,
        document_id: Uuid,
    ) -> Result<(), StoreError> {
        let res = sqlx::query("DELETE FROM dp_project_exec_summary_documents WHERE id = $1")
            .bind(document_id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("exec_summary_document", document_id));
        }
        Ok(())
    }

    // ----- changelog ---------------------------------------------------

    pub(super) async fn list_exec_summary_changelog_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ExecSummaryChangelogEntry>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, version, changed_at, changed_by, summary, created_at
                 FROM dp_project_exec_summary_changelog
                WHERE project_id = $1
                ORDER BY changed_at DESC, created_at DESC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_exec_summary_changelog).collect()
    }

    pub(super) async fn insert_exec_summary_changelog_impl(
        &self,
        insert: &ExecSummaryChangelogInsert,
    ) -> Result<ExecSummaryChangelogEntry, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_project_exec_summary_changelog
                   (project_id, version, changed_at, changed_by, summary)
               VALUES ($1, $2, $3, $4, $5)
            RETURNING id, project_id, version, changed_at, changed_by, summary, created_at"#,
        )
        .bind(insert.project_id)
        .bind(&insert.version)
        .bind(insert.changed_at)
        .bind(&insert.changed_by)
        .bind(&insert.summary)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_to_exec_summary_changelog(&row)
    }

    pub(super) async fn delete_exec_summary_changelog_impl(
        &self,
        entry_id: Uuid,
    ) -> Result<(), StoreError> {
        let res = sqlx::query("DELETE FROM dp_project_exec_summary_changelog WHERE id = $1")
            .bind(entry_id)
            .execute(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("exec_summary_changelog", entry_id));
        }
        Ok(())
    }
}

// ----- row decoders ----------------------------------------------------

fn row_to_exec_summary(r: &sqlx::postgres::PgRow) -> Result<ProjectExecSummary, StoreError> {
    let status_text: String = r.try_get("status").map_err(map_sqlx)?;
    let status = match status_text.as_str() {
        "draft" => ExecSummaryStatus::Draft,
        "in_review" => ExecSummaryStatus::InReview,
        "approved" => ExecSummaryStatus::Approved,
        other => return Err(invalid(format!("unknown exec summary status: {other}"))),
    };
    Ok(ProjectExecSummary {
        project_id: r.try_get("project_id").map_err(map_sqlx)?,

        product_name: r.try_get("product_name").map_err(map_sqlx)?,
        part_number: r.try_get("part_number").map_err(map_sqlx)?,
        target_release_date: r.try_get("target_release_date").map_err(map_sqlx)?,
        objective: r.try_get("objective").map_err(map_sqlx)?,
        problem: r.try_get("problem").map_err(map_sqlx)?,
        value: r.try_get("value").map_err(map_sqlx)?,
        differentiators: r.try_get("differentiators").map_err(map_sqlx)?,
        success_criteria: r.try_get("success_criteria").map_err(map_sqlx)?,

        in_scope: r.try_get("in_scope").map_err(map_sqlx)?,
        out_of_scope: r.try_get("out_of_scope").map_err(map_sqlx)?,
        assumptions: r.try_get("assumptions").map_err(map_sqlx)?,
        dependencies: r.try_get("dependencies").map_err(map_sqlx)?,
        constraints: r.try_get("constraints").map_err(map_sqlx)?,

        must_have: r.try_get("must_have").map_err(map_sqlx)?,
        optional: r.try_get("optional").map_err(map_sqlx)?,
        user_interaction: r.try_get("user_interaction").map_err(map_sqlx)?,
        architecture: r.try_get("architecture").map_err(map_sqlx)?,
        protocols: r.try_get("protocols").map_err(map_sqlx)?,
        power: r.try_get("power").map_err(map_sqlx)?,
        mounting: r.try_get("mounting").map_err(map_sqlx)?,
        certification: r.try_get("certification").map_err(map_sqlx)?,

        hardware_features: r.try_get("hardware_features").map_err(map_sqlx)?,
        physical_notes: r.try_get("physical_notes").map_err(map_sqlx)?,
        enclosure: r.try_get("enclosure").map_err(map_sqlx)?,
        mounting_type: r.try_get("mounting_type").map_err(map_sqlx)?,
        operating_env: r.try_get("operating_env").map_err(map_sqlx)?,

        rrp_cents: r.try_get("rrp_cents").map_err(map_sqlx)?,
        oem_price_cents: r.try_get("oem_price_cents").map_err(map_sqlx)?,
        target_gp_bp: r.try_get("target_gp_bp").map_err(map_sqlx)?,
        revenue_model: r.try_get("revenue_model").map_err(map_sqlx)?,
        channel_strategy: r.try_get("channel_strategy").map_err(map_sqlx)?,
        target_market: r.try_get("target_market").map_err(map_sqlx)?,
        volume_assumptions: r.try_get("volume_assumptions").map_err(map_sqlx)?,

        status,
        reviewer: r.try_get("reviewer").map_err(map_sqlx)?,
        approver: r.try_get("approver").map_err(map_sqlx)?,
        review_notes: r.try_get("review_notes").map_err(map_sqlx)?,
        approval_notes: r.try_get("approval_notes").map_err(map_sqlx)?,
        submitted_at: r.try_get("submitted_at").map_err(map_sqlx)?,
        approved_at: r.try_get("approved_at").map_err(map_sqlx)?,

        created_at: r.try_get("created_at").map_err(map_sqlx)?,
        updated_at: r.try_get("updated_at").map_err(map_sqlx)?,
    })
}

fn row_to_exec_summary_image(
    r: &sqlx::postgres::PgRow,
) -> Result<ExecSummaryImage, StoreError> {
    Ok(ExecSummaryImage {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        blob_ref: r.try_get::<JsonValue, _>("blob_ref").map_err(map_sqlx)?,
        filename: r.try_get("filename").map_err(map_sqlx)?,
        content_type: r.try_get("content_type").map_err(map_sqlx)?,
        caption: r.try_get("caption").map_err(map_sqlx)?,
        ord: r.try_get("ord").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
    })
}

fn row_to_exec_summary_document(
    r: &sqlx::postgres::PgRow,
) -> Result<ExecSummaryDocument, StoreError> {
    Ok(ExecSummaryDocument {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        blob_ref: r.try_get::<JsonValue, _>("blob_ref").map_err(map_sqlx)?,
        title: r.try_get("title").map_err(map_sqlx)?,
        doc_type: r.try_get("doc_type").map_err(map_sqlx)?,
        notes: r.try_get("notes").map_err(map_sqlx)?,
        required_action: r.try_get("required_action").map_err(map_sqlx)?,
        uploaded_by: r.try_get("uploaded_by").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
    })
}

fn row_to_exec_summary_changelog(
    r: &sqlx::postgres::PgRow,
) -> Result<ExecSummaryChangelogEntry, StoreError> {
    Ok(ExecSummaryChangelogEntry {
        id: r.try_get("id").map_err(map_sqlx)?,
        project_id: r.try_get("project_id").map_err(map_sqlx)?,
        version: r.try_get("version").map_err(map_sqlx)?,
        changed_at: r.try_get("changed_at").map_err(map_sqlx)?,
        changed_by: r.try_get("changed_by").map_err(map_sqlx)?,
        summary: r.try_get("summary").map_err(map_sqlx)?,
        created_at: r.try_get("created_at").map_err(map_sqlx)?,
    })
}
