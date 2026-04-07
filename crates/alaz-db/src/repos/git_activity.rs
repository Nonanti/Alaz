use alaz_core::Result;
use alaz_core::models::{CoupledFiles, HotFile};
use sqlx::PgPool;

pub struct GitActivityRepo;

impl GitActivityRepo {
    /// Record a batch of file changes from a single commit.
    pub async fn record_commit(
        pool: &PgPool,
        project_id: Option<&str>,
        commit_hash: &str,
        commit_message: &str,
        files: &[FileChange],
    ) -> Result<usize> {
        let mut count = 0;
        for file in files {
            let id = cuid2::create_id();
            sqlx::query(
                r#"
                INSERT INTO git_activity
                    (id, project_id, commit_hash, commit_message, file_path, change_type, lines_added, lines_removed)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(&id)
            .bind(project_id)
            .bind(commit_hash)
            .bind(commit_message)
            .bind(&file.path)
            .bind(&file.change_type)
            .bind(file.lines_added)
            .bind(file.lines_removed)
            .execute(pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    /// Get the most frequently changed files in the last N days.
    pub async fn hot_files(
        pool: &PgPool,
        project_id: Option<&str>,
        days: i32,
        limit: i64,
    ) -> Result<Vec<HotFile>> {
        let rows = sqlx::query_as::<_, HotFileRow>(
            r#"
            SELECT file_path,
                   COUNT(DISTINCT commit_hash) AS commit_count,
                   SUM(lines_added)::BIGINT AS total_lines_added,
                   SUM(lines_removed)::BIGINT AS total_lines_removed,
                   (SUM(lines_added) + SUM(lines_removed))::BIGINT AS total_churn
            FROM git_activity
            WHERE created_at > now() - make_interval(days => $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            GROUP BY file_path
            ORDER BY commit_count DESC, total_churn DESC
            LIMIT $3
            "#,
        )
        .bind(days)
        .bind(project_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| HotFile {
                file_path: r.file_path,
                commit_count: r.commit_count,
                total_lines_added: r.total_lines_added,
                total_lines_removed: r.total_lines_removed,
                total_churn: r.total_churn,
            })
            .collect())
    }

    /// Find files that frequently change together (temporal coupling).
    ///
    /// Returns pairs where both files appear in the same commit at least
    /// `min_co_changes` times within the last N days.
    pub async fn coupled_files(
        pool: &PgPool,
        project_id: Option<&str>,
        days: i32,
        min_co_changes: i64,
        limit: i64,
    ) -> Result<Vec<CoupledFiles>> {
        let rows = sqlx::query_as::<_, CoupledFilesRow>(
            r#"
            WITH recent AS (
                SELECT commit_hash, file_path
                FROM git_activity
                WHERE created_at > now() - make_interval(days => $1)
                  AND ($2::TEXT IS NULL OR project_id = $2)
            ),
            pairs AS (
                SELECT a.file_path AS file_a, b.file_path AS file_b,
                       COUNT(DISTINCT a.commit_hash) AS co_change_count
                FROM recent a
                JOIN recent b ON a.commit_hash = b.commit_hash AND a.file_path < b.file_path
                GROUP BY a.file_path, b.file_path
                HAVING COUNT(DISTINCT a.commit_hash) >= $3
            ),
            file_counts AS (
                SELECT file_path, COUNT(DISTINCT commit_hash) AS total_commits
                FROM recent
                GROUP BY file_path
            )
            SELECT p.file_a, p.file_b, p.co_change_count,
                   (p.co_change_count::FLOAT / GREATEST(fc.total_commits, 1))::FLOAT8 AS coupling_ratio
            FROM pairs p
            JOIN file_counts fc ON fc.file_path = p.file_a
            ORDER BY p.co_change_count DESC
            LIMIT $4
            "#,
        )
        .bind(days)
        .bind(project_id)
        .bind(min_co_changes)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CoupledFiles {
                file_a: r.file_a,
                file_b: r.file_b,
                co_change_count: r.co_change_count,
                coupling_ratio: r.coupling_ratio,
            })
            .collect())
    }
}

/// Input for recording a file change.
pub struct FileChange {
    pub path: String,
    pub change_type: String,
    pub lines_added: i32,
    pub lines_removed: i32,
}

/// Internal query result for hot files.
#[derive(sqlx::FromRow)]
struct HotFileRow {
    file_path: String,
    commit_count: i64,
    total_lines_added: i64,
    total_lines_removed: i64,
    total_churn: i64,
}

/// Internal query result for coupled files.
#[derive(sqlx::FromRow)]
struct CoupledFilesRow {
    file_a: String,
    file_b: String,
    co_change_count: i64,
    coupling_ratio: f64,
}
