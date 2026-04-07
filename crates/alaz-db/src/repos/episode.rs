use alaz_core::models::{CreateEpisode, Episode, ListEpisodesFilter};
use alaz_core::{AlazError, Result};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Standard SELECT columns for reading an Episode.
/// `type` is aliased to `kind` to match the Rust struct field.
const EPISODE_COLUMNS: &str = "\
    id, title, content, type AS kind, severity, resolved, \
    who_cues, what_cues, where_cues, when_cues, why_cues, \
    project_id, utility_score, access_count, last_accessed_at, needs_embedding, feedback_boost, \
    superseded_by, valid_from, valid_until, source, source_metadata, \
    action, outcome, outcome_score, related_files, created_at, updated_at";

/// Build a `SELECT <columns> FROM episodes <suffix>` query.
fn select_episodes(suffix: &str) -> String {
    format!("SELECT {EPISODE_COLUMNS} FROM episodes {suffix}")
}

pub struct EpisodeRepo;

impl EpisodeRepo {
    pub async fn create(
        pool: &PgPool,
        input: &CreateEpisode,
        project_id: Option<&str>,
    ) -> Result<Episode> {
        let id = cuid2::create_id();
        let kind = input.kind.as_deref().unwrap_or("discovery");
        let resolved = input.resolved.unwrap_or(false);
        let who_cues = input.who_cues.as_deref().unwrap_or(&[]);
        let what_cues = input.what_cues.as_deref().unwrap_or(&[]);
        let where_cues = input.where_cues.as_deref().unwrap_or(&[]);
        let when_cues = input.when_cues.as_deref().unwrap_or(&[]);
        let why_cues = input.why_cues.as_deref().unwrap_or(&[]);
        let related_files = input.related_files.as_deref().unwrap_or(&[]);

        let sql = format!(
            "INSERT INTO episodes (id, title, content, type, severity, resolved, \
                who_cues, what_cues, where_cues, when_cues, why_cues, project_id, \
                source, source_metadata, action, outcome, outcome_score, related_files) \
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) \
            RETURNING {EPISODE_COLUMNS}"
        );
        let row = sqlx::query_as::<_, Episode>(&sql)
            .bind(&id)
            .bind(&input.title)
            .bind(&input.content)
            .bind(kind)
            .bind(&input.severity)
            .bind(resolved)
            .bind(who_cues)
            .bind(what_cues)
            .bind(where_cues)
            .bind(when_cues)
            .bind(why_cues)
            .bind(project_id)
            .bind(input.source.as_deref().unwrap_or("pi"))
            .bind(
                input
                    .source_metadata
                    .as_ref()
                    .unwrap_or(&serde_json::json!({})),
            )
            .bind(&input.action)
            .bind(&input.outcome)
            .bind(input.outcome_score)
            .bind(related_files)
            .fetch_one(pool)
            .await?;

        Ok(row)
    }

    pub async fn get(pool: &PgPool, id: &str) -> Result<Episode> {
        let sql = select_episodes("WHERE id = $1");
        let row = sqlx::query_as::<_, Episode>(&sql)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AlazError::NotFound(format!("episode {id}")))?;

        Ok(row)
    }

    pub async fn delete(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM episodes WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("episode {id}")));
        }
        Ok(())
    }

    pub async fn list(pool: &PgPool, filter: &ListEpisodesFilter) -> Result<Vec<Episode>> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let sql = select_episodes(
            "WHERE ($1::TEXT IS NULL OR project_id = $1) \
              AND ($2::TEXT IS NULL OR type = $2) \
              AND ($3::BOOLEAN IS NULL OR resolved = $3) \
            ORDER BY created_at DESC \
            LIMIT $4 OFFSET $5",
        );
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(&filter.project)
            .bind(&filter.kind)
            .bind(filter.resolved)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Fetch multiple episodes by IDs in a single query.
    pub async fn get_many(pool: &PgPool, ids: &[String]) -> Result<Vec<Episode>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let sql = select_episodes("WHERE id = ANY($1)");
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(ids)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Search episodes by cue overlap using the array overlap `&&` operator.
    #[allow(clippy::too_many_arguments)]
    pub async fn cue_search(
        pool: &PgPool,
        who: Option<&[String]>,
        what: Option<&[String]>,
        where_: Option<&[String]>,
        when: Option<&[String]>,
        why: Option<&[String]>,
        project_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<Episode>> {
        let empty: Vec<String> = vec![];
        let who = who.unwrap_or(&empty);
        let what = what.unwrap_or(&empty);
        let where_cues = where_.unwrap_or(&empty);
        let when = when.unwrap_or(&empty);
        let why = why.unwrap_or(&empty);
        let limit = limit.unwrap_or(50);

        let sql = select_episodes(
            "WHERE (cardinality($1::TEXT[]) = 0 OR who_cues && $1) \
              AND (cardinality($2::TEXT[]) = 0 OR what_cues && $2) \
              AND (cardinality($3::TEXT[]) = 0 OR where_cues && $3) \
              AND (cardinality($4::TEXT[]) = 0 OR when_cues && $4) \
              AND (cardinality($5::TEXT[]) = 0 OR why_cues && $5) \
              AND ($6::TEXT IS NULL OR project_id = $6) \
            ORDER BY created_at DESC \
            LIMIT $7",
        );
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(who)
            .bind(what)
            .bind(where_cues)
            .bind(when)
            .bind(why)
            .bind(project_id)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    pub async fn find_needing_embedding(pool: &PgPool, limit: i64) -> Result<Vec<Episode>> {
        let sql = select_episodes("WHERE needs_embedding = TRUE ORDER BY created_at ASC LIMIT $1");
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Record an access event for an episode (increment count, update timestamp).
    ///
    /// Returns [`AlazError::NotFound`] if the episode does not exist.
    pub async fn record_access(pool: &PgPool, id: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE episodes SET access_count = access_count + 1, last_accessed_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AlazError::NotFound(format!("episode {id}")));
        }
        Ok(())
    }

    pub async fn mark_embedded(pool: &PgPool, id: &str) -> Result<()> {
        sqlx::query("UPDATE episodes SET needs_embedding = FALSE WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// List episodes within a date range, ordered by created_at DESC.
    pub async fn list_in_range(
        pool: &PgPool,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        project_id: Option<&str>,
    ) -> Result<Vec<Episode>> {
        let sql = select_episodes(
            "WHERE created_at >= $1 AND created_at <= $2 \
              AND ($3::TEXT IS NULL OR project_id = $3) \
            ORDER BY created_at DESC",
        );
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(start)
            .bind(end)
            .bind(project_id)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Find episodes with similar titles using trigram similarity.
    pub async fn find_similar_by_title(
        pool: &PgPool,
        title: &str,
        threshold: f32,
        project_id: Option<&str>,
    ) -> Result<Vec<Episode>> {
        let sql = select_episodes(
            "WHERE similarity(title, $1) > $2 \
              AND ($3::TEXT IS NULL OR project_id = $3) \
            ORDER BY similarity(title, $1) DESC \
            LIMIT 5",
        );
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(title)
            .bind(threshold)
            .bind(project_id)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }

    /// Mark an episode as superseded by a newer one.
    pub async fn supersede(pool: &PgPool, old_id: &str, new_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE episodes
            SET superseded_by = $2,
                valid_until = now(),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(old_id)
        .bind(new_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Find episodes related to specific files using array overlap.
    pub async fn find_by_files(
        pool: &PgPool,
        file_paths: &[&str],
        limit: i64,
    ) -> Result<Vec<Episode>> {
        if file_paths.is_empty() {
            return Ok(vec![]);
        }
        let sql = select_episodes("WHERE related_files && $1 ORDER BY created_at DESC LIMIT $2");
        let rows = sqlx::query_as::<_, Episode>(&sql)
            .bind(file_paths)
            .bind(limit)
            .fetch_all(pool)
            .await?;

        Ok(rows)
    }
}
