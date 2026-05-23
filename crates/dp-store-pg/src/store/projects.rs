use dp_domain::project::{
    PortfolioQueryFilter, PortfolioRawRow, Project, ProjectIssueAddOutcome, ProjectIssueAddSkip,
    ProjectListFilter, ProjectRepo, ProjectStatus, ProjectUpsert,
};
use dp_domain::project_view::{
    ProjectView, ProjectViewUpsert,
};
use dp_domain::store::StoreError;
use sqlx::Row;
use uuid::Uuid;


use super::{invalid, map_sqlx, not_found, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn list_projects_impl(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<Vec<Project>, StoreError> {
        let q_norm = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let rows = sqlx::query(
            r#"SELECT id, org_id, name, description, lead_user_id, status,
                      start_at, due_at, issue_count, closed_issue_count,
                      created_by, created_at, updated_at, version,
                      primary_milestone_id
                 FROM dp_projects
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%')
             ORDER BY
                  CASE status
                      WHEN 'active'   THEN 0
                      WHEN 'backlog'  THEN 1
                      WHEN 'done'     THEN 2
                      WHEN 'archived' THEN 3
                  END,
                  due_at ASC NULLS LAST,
                  name ASC
                LIMIT $4 OFFSET $5"#,
        )
        .bind(filter.org_id)
        .bind(status_text)
        .bind(q_norm)
        .bind(filter.limit)
        .bind(filter.offset)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_project).collect()
    }

    pub(super) async fn count_projects_impl(
        &self,
        filter: &ProjectListFilter,
    ) -> Result<i64, StoreError> {
        let q_norm = filter
            .q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let status_text = filter.status.map(|s| s.as_str().to_string());
        let (count,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint
                 FROM dp_projects
                WHERE ($1::uuid IS NULL OR org_id = $1)
                  AND ($2::text IS NULL OR status = $2)
                  AND ($3::text IS NULL OR name ILIKE '%' || $3 || '%')"#,
        )
        .bind(filter.org_id)
        .bind(status_text)
        .bind(q_norm)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(count)
    }

    pub(super) async fn list_project_portfolio_impl(
        &self,
        filter: &PortfolioQueryFilter,
    ) -> Result<Vec<PortfolioRawRow>, StoreError> {
        let sql = dp_reports::build_project_portfolio_sql(filter.sort);
        let statuses: Vec<String> = filter
            .statuses
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();
        let (window_start, window_end) = match filter.window {
            Some((s, e)) => (Some(s), Some(e)),
            None => (None, None),
        };
        let rows = sqlx::query(&sql)
            .bind(&filter.orgs)
            .bind(&statuses)
            .bind(window_start)
            .bind(window_end)
            .bind(filter.hide_overdue)
            .bind(filter.now)
            .bind(filter.limit)
            .bind(filter.offset)
            .fetch_all(self.pool.sqlx())
            .await
            .map_err(map_sqlx)?;
        rows.iter().map(row_to_portfolio_raw).collect()
    }

    pub(super) async fn get_project_impl(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            r#"SELECT id, org_id, name, description, lead_user_id, status,
                      start_at, due_at, issue_count, closed_issue_count,
                      created_by, created_at, updated_at, version,
                      primary_milestone_id
                 FROM dp_projects WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_project(&r)).transpose()
    }

    pub(super) async fn create_project_impl(
        &self,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        let row = sqlx::query(
            r#"INSERT INTO dp_projects
                   (id, org_id, name, description, lead_user_id, status,
                    start_at, due_at, created_by, created_at, updated_at, version)
               VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8,
                       now(), now(), 1)
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(upsert.org_id)
        .bind(&upsert.name)
        .bind(upsert.description.as_deref())
        .bind(upsert.lead_user_id)
        .bind(upsert.status.as_str())
        .bind(upsert.start_at)
        .bind(upsert.due_at)
        .bind(upsert.created_by)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db)
                if db.constraint().is_some()
                    && (db.message().contains("dp_projects_check")
                        || db.message().contains("dp_projects")
                            && db.message().contains("check")) =>
            {
                invalid("project violates a CHECK constraint (status / dates / counts)")
            }
            _ => map_sqlx(e),
        })?;
        row_to_project(&row)
    }

    pub(super) async fn update_project_impl(
        &self,
        id: Uuid,
        expected_version: i64,
        upsert: &ProjectUpsert,
    ) -> Result<Project, StoreError> {
        // §8.2 CAS: WHERE id = ? AND version = ?. A miss is either
        // "row gone" (NotFound) or "stale version" (Conflict). One
        // extra SELECT distinguishes them; cheaper than a serializable
        // transaction and lets the REST layer pick its 404 vs 409.
        let row = sqlx::query(
            r#"UPDATE dp_projects
                  SET name         = $3,
                      description  = $4,
                      lead_user_id = $5,
                      status       = $6,
                      start_at     = $7,
                      due_at       = $8,
                      version      = version + 1,
                      updated_at   = now()
                WHERE id = $1 AND version = $2
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(id)
        .bind(expected_version)
        .bind(&upsert.name)
        .bind(upsert.description.as_deref())
        .bind(upsert.lead_user_id)
        .bind(upsert.status.as_str())
        .bind(upsert.start_at)
        .bind(upsert.due_at)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_project(&r),
            None => disambiguate_project_miss(self, id).await,
        }
    }

    pub(super) async fn archive_project_impl(
        &self,
        id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        // Idempotent: archiving an already-archived row returns the
        // row as-is without a version bump (§9.2 wording). Anything
        // else CAS-gates on version.
        let current = self.get_project_impl(id).await?;
        let Some(current) = current else {
            return Err(not_found("project", id));
        };
        if current.status == ProjectStatus::Archived {
            // No-op: caller's expected_version may even be stale but
            // there is nothing to bump. Return the row unchanged.
            return Ok(current);
        }
        let row = sqlx::query(
            r#"UPDATE dp_projects
                  SET status     = 'archived',
                      version    = version + 1,
                      updated_at = now()
                WHERE id = $1 AND version = $2
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(id)
        .bind(expected_version)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row {
            Some(r) => row_to_project(&r),
            None => disambiguate_project_miss(self, id).await,
        }
    }

    pub(super) async fn add_issues_to_project_impl(
        &self,
        project_id: Uuid,
        expected_version: i64,
        issue_ids: &[Uuid],
        actor: Option<Uuid>,
    ) -> Result<ProjectIssueAddOutcome, StoreError> {
        // One transaction so a concurrent writer cannot observe the
        // half-bumped counts or race the version gate. `FOR UPDATE`
        // serialises against any other writer touching this project
        // row.
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let project_row: Option<(Uuid, i64, String)> = sqlx::query_as(
            "SELECT org_id, version, status FROM dp_projects WHERE id = $1 FOR UPDATE",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let (project_org, current_version, _status) = match project_row {
            Some(r) => r,
            None => return Err(not_found("project", project_id)),
        };
        if current_version != expected_version {
            return Err(StoreError::Conflict(format!(
                "project version mismatch: expected {expected_version}, found {current_version}"
            )));
        }

        let mut added: Vec<Uuid> = Vec::new();
        let mut skipped: Vec<ProjectIssueAddSkip> = Vec::new();

        for &issue_id in issue_ids {
            // Resolve the issue plus its current membership (if any)
            // in a single round-trip so the per-row decision below
            // doesn't need a second query.
            let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
                r#"SELECT i.org_id, pi.project_id
                     FROM dp_issues i
                     LEFT JOIN dp_project_issues pi ON pi.issue_id = i.id
                    WHERE i.id = $1"#,
            )
            .bind(issue_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sqlx)?;

            let Some((issue_org, existing_project)) = row else {
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "unknown_issue".into(),
                    existing_project_id: None,
                });
                continue;
            };
            if issue_org != project_org {
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "cross_org".into(),
                    existing_project_id: None,
                });
                continue;
            }
            if let Some(existing) = existing_project {
                // Already attached — either to this project (idempotent
                // re-add) or to another. v1 collapses both to
                // `already_in_project`; the existing project id lets
                // the UI offer `Move here?` when it's a different one.
                skipped.push(ProjectIssueAddSkip {
                    issue_id,
                    reason: "already_in_project".into(),
                    existing_project_id: Some(existing),
                });
                continue;
            }

            sqlx::query(
                r#"INSERT INTO dp_project_issues (project_id, issue_id, added_by, added_at)
                       VALUES ($1, $2, $3, now())"#,
            )
            .bind(project_id)
            .bind(issue_id)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
            added.push(issue_id);
        }

        // Recompute counts + bump version inside the same tx so the
        // returned outcome reflects committed state. We only bump
        // `version` when at least one issue was added — the §7.2
        // contract.
        if !added.is_empty() {
            sqlx::query(
                r#"UPDATE dp_projects p
                      SET issue_count = (
                              SELECT COUNT(*) FROM dp_project_issues
                               WHERE project_id = p.id),
                          closed_issue_count = (
                              SELECT COUNT(*)
                                FROM dp_project_issues pi
                                JOIN dp_issues i ON i.id = pi.issue_id
                               WHERE pi.project_id = p.id AND i.state = 'closed'),
                          version    = version + 1,
                          updated_at = now()
                    WHERE id = $1"#,
            )
            .bind(project_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }

        tx.commit().await.map_err(map_sqlx)?;

        Ok(ProjectIssueAddOutcome { added, skipped })
    }

    pub(super) async fn remove_issue_from_project_impl(
        &self,
        project_id: Uuid,
        issue_id: Uuid,
        expected_version: i64,
    ) -> Result<Project, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;

        let project_row: Option<(i64,)> = sqlx::query_as(
            "SELECT version FROM dp_projects WHERE id = $1 FOR UPDATE",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let current_version = match project_row {
            Some((v,)) => v,
            None => return Err(not_found("project", project_id)),
        };
        if current_version != expected_version {
            return Err(StoreError::Conflict(format!(
                "project version mismatch: expected {expected_version}, found {current_version}"
            )));
        }

        let res = sqlx::query(
            "DELETE FROM dp_project_issues WHERE project_id = $1 AND issue_id = $2",
        )
        .bind(project_id)
        .bind(issue_id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("project_issue", issue_id));
        }

        let row = sqlx::query(
            r#"UPDATE dp_projects p
                  SET issue_count = (
                          SELECT COUNT(*) FROM dp_project_issues
                           WHERE project_id = p.id),
                      closed_issue_count = (
                          SELECT COUNT(*)
                            FROM dp_project_issues pi
                            JOIN dp_issues i ON i.id = pi.issue_id
                           WHERE pi.project_id = p.id AND i.state = 'closed'),
                      version    = version + 1,
                      updated_at = now()
                WHERE id = $1
               RETURNING id, org_id, name, description, lead_user_id, status,
                         start_at, due_at, issue_count, closed_issue_count,
                         created_by, created_at, updated_at, version,
                         primary_milestone_id"#,
        )
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let project = row_to_project(&row)?;
        tx.commit().await.map_err(map_sqlx)?;
        Ok(project)
    }

    pub(super) async fn get_project_for_issue_impl(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            r#"SELECT p.id, p.org_id, p.name, p.description, p.lead_user_id, p.status,
                      p.start_at, p.due_at, p.issue_count, p.closed_issue_count,
                      p.created_by, p.created_at, p.updated_at, p.version,
                      p.primary_milestone_id
                 FROM dp_projects p
                 JOIN dp_project_issues pi ON pi.project_id = p.id
                WHERE pi.issue_id = $1"#,
        )
        .bind(issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row.map(|r| row_to_project(&r)).transpose()
    }

    pub(super) async fn list_issue_ids_for_project_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT issue_id
                 FROM dp_project_issues
                WHERE project_id = $1
             ORDER BY added_at ASC, issue_id ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| r.try_get::<Uuid, _>("issue_id").map_err(map_sqlx))
            .collect()
    }

    pub(super) async fn list_project_issue_tag_values_impl(
        &self,
        project_id: Uuid,
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        // Walk the project's issue ids through dp_tag_links → dp_tags
        // and pull `(issue_id, value)` for the requested kv key.
        // Archived tags are excluded so the workbench's bucket list
        // tracks live data only (PROJECT-VIEW.md §5.1).
        let rows = sqlx::query(
            r#"SELECT tl.target_issue_id AS issue_id, t.value AS value
                 FROM dp_project_issues pi
                 JOIN dp_tag_links tl ON tl.target_issue_id = pi.issue_id
                                     AND tl.kind = 'issue'
                 JOIN dp_tags t       ON t.id = tl.tag_id
                                     AND t.kind = 'kv'
                                     AND t.key = $2
                                     AND t.archived_at IS NULL
                WHERE pi.project_id = $1"#,
        )
        .bind(project_id)
        .bind(tag_key)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| {
                let id: Uuid = r.try_get("issue_id").map_err(map_sqlx)?;
                let v: String = r.try_get("value").map_err(map_sqlx)?;
                Ok((id, v))
            })
            .collect()
    }

    pub(super) async fn list_issue_tag_values_impl(
        &self,
        issue_ids: &[Uuid],
        tag_key: &str,
    ) -> Result<Vec<(Uuid, String)>, StoreError> {
        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            r#"SELECT tl.target_issue_id AS issue_id, t.value AS value
                 FROM dp_tag_links tl
                 JOIN dp_tags t ON t.id = tl.tag_id
                                AND t.kind = 'kv'
                                AND t.key = $2
                                AND t.archived_at IS NULL
                WHERE tl.kind = 'issue'
                  AND tl.target_issue_id = ANY($1)"#,
        )
        .bind(issue_ids)
        .bind(tag_key)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| {
                let id: Uuid = r.try_get("issue_id").map_err(map_sqlx)?;
                let v: String = r.try_get("value").map_err(map_sqlx)?;
                Ok((id, v))
            })
            .collect()
    }

    pub(super) async fn list_project_issue_tag_keys_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT DISTINCT t.key AS key
                 FROM dp_project_issues pi
                 JOIN dp_tag_links tl ON tl.target_issue_id = pi.issue_id
                                     AND tl.kind = 'issue'
                 JOIN dp_tags t       ON t.id = tl.tag_id
                                     AND t.kind = 'kv'
                                     AND t.archived_at IS NULL
                                     AND t.key IS NOT NULL
                WHERE pi.project_id = $1
             ORDER BY key ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter()
            .map(|r| r.try_get::<String, _>("key").map_err(map_sqlx))
            .collect()
    }

    pub(super) async fn list_project_views_impl(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Vec<ProjectView>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
             ORDER BY position ASC, created_at ASC"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(project_view_from_row).collect()
    }

    pub(super) async fn get_project_view_impl(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<ProjectView>, StoreError> {
        let row_opt = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE id = $1 AND owner_user_id = $2"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        row_opt.as_ref().map(project_view_from_row).transpose()
    }

    pub(super) async fn create_project_view_impl(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        let id = Uuid::new_v4();
        let filter_json = serde_json::to_value(&upsert.filter_clauses)
            .map_err(|e| StoreError::Invalid(format!("filter_json encode: {e}")))?;
        let categories_json = serde_json::to_value(&upsert.categories)
            .map_err(|e| StoreError::Invalid(format!("categories encode: {e}")))?;
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Append-at-end position. Per-(project, owner) so two users'
        // tab strips never collide on position.
        let (next_pos,): (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let row = sqlx::query(
            r#"INSERT INTO dp_project_views
                  (id, project_id, owner_user_id, name, group_by,
                   filter_json, sort, position, visibility,
                   start_date, due_date, categories)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
               RETURNING id, project_id, owner_user_id, name, group_by,
                         filter_json, sort, position, visibility,
                         start_date, due_date, categories,
                         created_at, updated_at"#,
        )
        .bind(id)
        .bind(project_id)
        .bind(owner_user_id)
        .bind(&upsert.name)
        .bind(&upsert.group_by)
        .bind(&filter_json)
        .bind(if upsert.sort.is_empty() {
            "updated_desc"
        } else {
            upsert.sort.as_str()
        })
        .bind(next_pos as i32)
        .bind(upsert.visibility.as_str())
        .bind(upsert.start_date)
        .bind(upsert.due_date)
        .bind(&categories_json)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        project_view_from_row(&row)
    }

    pub(super) async fn update_project_view_impl(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
        upsert: &ProjectViewUpsert,
    ) -> Result<ProjectView, StoreError> {
        let filter_json = serde_json::to_value(&upsert.filter_clauses)
            .map_err(|e| StoreError::Invalid(format!("filter_json encode: {e}")))?;
        let categories_json = serde_json::to_value(&upsert.categories)
            .map_err(|e| StoreError::Invalid(format!("categories encode: {e}")))?;
        let row_opt = sqlx::query(
            r#"UPDATE dp_project_views
                  SET name = $3,
                      group_by = $4,
                      filter_json = $5,
                      sort = $6,
                      visibility = $7,
                      start_date = $8,
                      due_date = $9,
                      categories = $10,
                      updated_at = now()
                WHERE id = $1 AND owner_user_id = $2
                RETURNING id, project_id, owner_user_id, name, group_by,
                          filter_json, sort, position, visibility,
                          start_date, due_date, categories,
                          created_at, updated_at"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .bind(&upsert.name)
        .bind(&upsert.group_by)
        .bind(&filter_json)
        .bind(if upsert.sort.is_empty() {
            "updated_desc"
        } else {
            upsert.sort.as_str()
        })
        .bind(upsert.visibility.as_str())
        .bind(upsert.start_date)
        .bind(upsert.due_date)
        .bind(&categories_json)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        match row_opt {
            Some(r) => project_view_from_row(&r),
            None => Err(not_found("project_view", id)),
        }
    }

    pub(super) async fn delete_project_view_impl(
        &self,
        id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<(), StoreError> {
        let res = sqlx::query(
            r#"DELETE FROM dp_project_views
                WHERE id = $1 AND owner_user_id = $2"#,
        )
        .bind(id)
        .bind(owner_user_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("project_view", id));
        }
        Ok(())
    }

    pub(super) async fn reorder_project_views_impl(
        &self,
        project_id: Uuid,
        owner_user_id: Uuid,
        ordered_ids: &[Uuid],
    ) -> Result<Vec<ProjectView>, StoreError> {
        let mut tx = self.pool.sqlx().begin().await.map_err(map_sqlx)?;
        // Lock the caller's views so a concurrent reorder / create
        // can't shift the set out from under us.
        let existing: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT id FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
                FOR UPDATE"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        let existing_set: std::collections::HashSet<Uuid> =
            existing.into_iter().map(|(i,)| i).collect();
        let req_set: std::collections::HashSet<Uuid> =
            ordered_ids.iter().copied().collect();
        if existing_set != req_set {
            return Err(StoreError::Invalid(
                "reorder ordered_ids must match the existing view set".into(),
            ));
        }
        // Two-phase rewrite to dodge the UNIQUE on (project_id,
        // owner_user_id, position) — none exists today but if it's
        // added the swap-via-negatives keeps us safe.
        for (idx, vid) in ordered_ids.iter().enumerate() {
            sqlx::query(
                r#"UPDATE dp_project_views
                      SET position = $3, updated_at = now()
                    WHERE id = $1 AND owner_user_id = $2"#,
            )
            .bind(vid)
            .bind(owner_user_id)
            .bind(-(idx as i32) - 1)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        for (idx, vid) in ordered_ids.iter().enumerate() {
            sqlx::query(
                r#"UPDATE dp_project_views
                      SET position = $3
                    WHERE id = $1 AND owner_user_id = $2"#,
            )
            .bind(vid)
            .bind(owner_user_id)
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx)?;
        }
        let rows = sqlx::query(
            r#"SELECT id, project_id, owner_user_id, name, group_by,
                      filter_json, sort, position, visibility,
                      start_date, due_date, categories,
                      created_at, updated_at
                 FROM dp_project_views
                WHERE project_id = $1 AND owner_user_id = $2
             ORDER BY position ASC"#,
        )
        .bind(project_id)
        .bind(owner_user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx)?;
        tx.commit().await.map_err(map_sqlx)?;
        rows.iter().map(project_view_from_row).collect()
    }

    pub(super) async fn list_issue_ids_for_view_impl(
        &self,
        view_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            r#"SELECT issue_id
                 FROM dp_project_view_issues
                WHERE view_id = $1
             ORDER BY added_at ASC, issue_id ASC"#,
        )
        .bind(view_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(rows.into_iter().map(|(i,)| i).collect())
    }

    pub(super) async fn add_issues_to_view_impl(
        &self,
        view_id: Uuid,
        issue_ids: &[Uuid],
    ) -> Result<(), StoreError> {
        if issue_ids.is_empty() {
            return Ok(());
        }
        // One round-trip via UNNEST; ON CONFLICT keeps the call
        // idempotent so retries after a partial network failure
        // don't churn `added_at`.
        sqlx::query(
            r#"INSERT INTO dp_project_view_issues (view_id, issue_id)
                    SELECT $1, x.issue_id
                      FROM UNNEST($2::uuid[]) AS x(issue_id)
                ON CONFLICT (view_id, issue_id) DO NOTHING"#,
        )
        .bind(view_id)
        .bind(issue_ids)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn remove_issue_from_view_impl(
        &self,
        view_id: Uuid,
        issue_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"DELETE FROM dp_project_view_issues
                WHERE view_id = $1 AND issue_id = $2"#,
        )
        .bind(view_id)
        .bind(issue_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn list_project_repos_impl(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectRepo>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT project_id, repo_id, added_by, added_at
                 FROM dp_project_repos
                WHERE project_id = $1
             ORDER BY added_at ASC, repo_id ASC"#,
        )
        .bind(project_id)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.into_iter()
            .map(|r| {
                Ok(ProjectRepo {
                    project_id: r.try_get("project_id").map_err(map_sqlx)?,
                    repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
                    added_by: r.try_get("added_by").map_err(map_sqlx)?,
                    added_at: r.try_get("added_at").map_err(map_sqlx)?,
                })
            })
            .collect()
    }

    pub(super) async fn add_project_repo_impl(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
        actor: Option<Uuid>,
    ) -> Result<ProjectRepo, StoreError> {
        // ON CONFLICT DO NOTHING + RETURNING returns no row for an
        // existing PK; fall back to a SELECT so callers see the
        // pre-existing row.
        let row_opt = sqlx::query(
            r#"INSERT INTO dp_project_repos (project_id, repo_id, added_by)
               VALUES ($1, $2, $3)
               ON CONFLICT (project_id, repo_id) DO NOTHING
               RETURNING project_id, repo_id, added_by, added_at"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .bind(actor)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        if let Some(r) = row_opt {
            return Ok(ProjectRepo {
                project_id: r.try_get("project_id").map_err(map_sqlx)?,
                repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
                added_by: r.try_get("added_by").map_err(map_sqlx)?,
                added_at: r.try_get("added_at").map_err(map_sqlx)?,
            });
        }
        let r = sqlx::query(
            r#"SELECT project_id, repo_id, added_by, added_at
                 FROM dp_project_repos
                WHERE project_id = $1 AND repo_id = $2"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(ProjectRepo {
            project_id: r.try_get("project_id").map_err(map_sqlx)?,
            repo_id: r.try_get("repo_id").map_err(map_sqlx)?,
            added_by: r.try_get("added_by").map_err(map_sqlx)?,
            added_at: r.try_get("added_at").map_err(map_sqlx)?,
        })
    }

    pub(super) async fn remove_project_repo_impl(
        &self,
        project_id: Uuid,
        repo_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"DELETE FROM dp_project_repos
                WHERE project_id = $1 AND repo_id = $2"#,
        )
        .bind(project_id)
        .bind(repo_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }
}

/// Resolve whether an `UPDATE dp_projects ... WHERE id = ? AND
/// version = ?` that affected zero rows was caused by the row going
/// away (NotFound) or by a stale `expected_version` (Conflict).
/// Pulled out so `update_project` and `archive_project` share one
/// place that picks the right `StoreError` variant.
pub(super) async fn disambiguate_project_miss(
    store: &PgStore,
    id: Uuid,
) -> Result<Project, StoreError> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT version FROM dp_projects WHERE id = $1")
            .bind(id)
            .fetch_optional(store.pool().sqlx())
            .await
            .map_err(map_sqlx)?;
    match existing {
        Some((v,)) => Err(StoreError::Conflict(format!(
            "project version mismatch: row currently at version {v}"
        ))),
        None => Err(not_found("project", id)),
    }
}
