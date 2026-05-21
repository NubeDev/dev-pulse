-- 0027_project_repos.sql
--
-- Project → repo associations. A project can be linked to one or
-- more repos so the §6.3 "Add issues" dialog (and any future repo-
-- scoped filter on the project detail page) can narrow the issue
-- picker to repos the operator has explicitly associated with the
-- project. This is purely an organizational / filtering aid — it
-- does NOT gate which issues the project can hold (issues from
-- non-linked repos can still be added via direct
-- `POST /projects/{id}/issues`); the spec calls this "soft scoping"
-- so a roadmap project that occasionally pulls in cross-repo work
-- isn't blocked.
--
-- Many-to-many. The natural key `(project_id, repo_id)` is the
-- primary key; no surrogate row id is needed because the §7
-- handlers take both ids on the URL.
--
-- Org constraint is enforced at the application layer (project
-- and repo must share the same `org_id`). A trigger would be
-- correct but adds maintenance cost for a check the handler
-- already runs; the FK CASCADE on `dp_orgs` keeps the table
-- consistent if an org is deleted.

CREATE TABLE dp_project_repos (
  project_id  uuid        NOT NULL REFERENCES dp_projects(id) ON DELETE CASCADE,
  repo_id     uuid        NOT NULL REFERENCES dp_repos(id)    ON DELETE CASCADE,
  added_by    uuid        REFERENCES dp_users(id) ON DELETE SET NULL,
  added_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (project_id, repo_id)
);

-- Reverse-lookup index for the §6.3 "issues for this project's
-- linked repos" filter, which fans the project's repo list out
-- against `dp_issues.repo_id`.
CREATE INDEX dp_project_repos_repo_idx ON dp_project_repos (repo_id);
