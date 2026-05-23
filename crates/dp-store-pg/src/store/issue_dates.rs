
use chrono::{DateTime, Utc};
use dp_domain::issue_dates::{IssueDates, ProjectV2MirrorTask, ProjectV2MirrorTaskKind};
use dp_domain::store::{
    IssueDatesMirrorOutcome, StoreError,
};
use uuid::Uuid;


use super::{invalid, map_sqlx, PgStore};
use super::rows::*;


impl PgStore {

    pub(super) async fn get_issue_dates_impl(
        &self,
        issue_id: Uuid,
    ) -> Result<Option<IssueDates>, StoreError> {
        let row = sqlx::query(
            r#"SELECT issue_id, start_at, due_at, mirror_node_id,
                      mirror_synced_at, mirror_error, updated_at
                 FROM dp_issue_dates WHERE issue_id = $1"#,
        )
        .bind(issue_id)
        .fetch_optional(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(row.map(|r| row_to_issue_dates(&r)).transpose()?)
    }

    pub(super) async fn upsert_issue_dates_impl(
        &self,
        issue_id: Uuid,
        start_at: Option<DateTime<Utc>>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<IssueDates, StoreError> {
        // The CHECK on the table guards start <= due; surface a
        // violation as Invalid so the handler can return 400
        // rather than a generic backend error.
        let row = sqlx::query(
            r#"
            INSERT INTO dp_issue_dates (issue_id, start_at, due_at, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (issue_id) DO UPDATE
              SET start_at  = EXCLUDED.start_at,
                  due_at    = EXCLUDED.due_at,
                  updated_at = now()
            RETURNING issue_id, start_at, due_at, mirror_node_id,
                      mirror_synced_at, mirror_error, updated_at
            "#,
        )
        .bind(issue_id)
        .bind(start_at)
        .bind(due_at)
        .fetch_one(self.pool.sqlx())
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db)
                if db.constraint().is_some()
                    && db.message().contains("dp_issue_dates_check") =>
            {
                invalid("start_at must be <= due_at")
            }
            _ => map_sqlx(e),
        })?;
        row_to_issue_dates(&row)
    }

    pub(super) async fn record_issue_dates_mirror_result_impl(
        &self,
        issue_id: Uuid,
        outcome: IssueDatesMirrorOutcome<'_>,
    ) -> Result<(), StoreError> {
        match outcome {
            IssueDatesMirrorOutcome::Success { node_id } => {
                sqlx::query(
                    r#"UPDATE dp_issue_dates
                          SET mirror_node_id   = COALESCE($2, mirror_node_id),
                              mirror_synced_at = now(),
                              mirror_error     = NULL
                        WHERE issue_id = $1"#,
                )
                .bind(issue_id)
                .bind(node_id)
                .execute(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
            }
            IssueDatesMirrorOutcome::Failure { error } => {
                sqlx::query(
                    r#"UPDATE dp_issue_dates
                          SET mirror_error = $2
                        WHERE issue_id = $1"#,
                )
                .bind(issue_id)
                .bind(error)
                .execute(self.pool.sqlx())
                .await
                .map_err(map_sqlx)?;
            }
        }
        Ok(())
    }

    pub(super) async fn set_issue_github_node_id_impl(
        &self,
        issue_id: Uuid,
        node_id: &str,
    ) -> Result<(), StoreError> {
        // Only stamp when currently NULL — the column is immutable
        // once known. A racing webhook upsert that observes the
        // same value is a harmless no-op.
        sqlx::query(
            r#"UPDATE dp_issues
                  SET github_node_id = $2
                WHERE id = $1
                  AND github_node_id IS NULL"#,
        )
        .bind(issue_id)
        .bind(node_id)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn enqueue_projectv2_mirror_task_impl(
        &self,
        issue_id: Uuid,
        repo_id: Uuid,
        kind: ProjectV2MirrorTaskKind,
        payload: serde_json::Value,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"INSERT INTO dp_projectv2_mirror_tasks
                   (issue_id, repo_id, kind, payload)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(issue_id)
        .bind(repo_id)
        .bind(kind.as_str())
        .bind(payload)
        .execute(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        Ok(())
    }

    pub(super) async fn claim_projectv2_mirror_tasks_impl(
        &self,
        max: i64,
    ) -> Result<Vec<ProjectV2MirrorTask>, StoreError> {
        let rows = sqlx::query(
            r#"SELECT id, issue_id, repo_id, kind, payload, attempts,
                      last_error, enqueued_at, processed_at
                 FROM dp_projectv2_mirror_tasks
                WHERE processed_at IS NULL
             ORDER BY enqueued_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED"#,
        )
        .bind(max)
        .fetch_all(self.pool.sqlx())
        .await
        .map_err(map_sqlx)?;
        rows.iter().map(row_to_projectv2_mirror_task).collect()
    }
}
