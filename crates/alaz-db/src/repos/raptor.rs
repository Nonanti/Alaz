use alaz_core::models::{RaptorNode, RaptorTree};
use alaz_core::{AlazError, Result};
use sqlx::PgPool;

pub struct RaptorRepo;

impl RaptorRepo {
    /// Create or update a raptor tree for a project. Uses ON CONFLICT on project_id.
    /// Pass None for global (cross-project) RAPTOR tree.
    pub async fn upsert_tree(pool: &PgPool, project_id: Option<&str>) -> Result<RaptorTree> {
        let id = cuid2::create_id();

        let row = if project_id.is_some() {
            sqlx::query_as::<_, RaptorTree>(
                r#"
                INSERT INTO raptor_trees (id, project_id, status)
                VALUES ($1, $2, 'building')
                ON CONFLICT (project_id) DO UPDATE SET
                    status = 'building',
                    updated_at = now()
                RETURNING id, project_id, status, total_nodes, max_depth, created_at, updated_at
                "#,
            )
            .bind(&id)
            .bind(project_id)
            .fetch_one(pool)
            .await?
        } else {
            // For global tree (NULL project_id), check if one exists first.
            // PostgreSQL UNIQUE doesn't cover NULLs, so we use a SELECT + INSERT/UPDATE pattern.
            let existing = sqlx::query_as::<_, RaptorTree>(
                r#"
                SELECT id, project_id, status, total_nodes, max_depth, created_at, updated_at
                FROM raptor_trees WHERE project_id IS NULL
                LIMIT 1
                "#,
            )
            .fetch_optional(pool)
            .await?;

            if let Some(tree) = existing {
                sqlx::query_as::<_, RaptorTree>(
                    r#"
                    UPDATE raptor_trees
                    SET status = 'building', updated_at = now()
                    WHERE id = $1
                    RETURNING id, project_id, status, total_nodes, max_depth, created_at, updated_at
                    "#,
                )
                .bind(&tree.id)
                .fetch_one(pool)
                .await?
            } else {
                sqlx::query_as::<_, RaptorTree>(
                    r#"
                    INSERT INTO raptor_trees (id, project_id, status)
                    VALUES ($1, NULL, 'building')
                    RETURNING id, project_id, status, total_nodes, max_depth, created_at, updated_at
                    "#,
                )
                .bind(&id)
                .fetch_one(pool)
                .await?
            }
        };

        Ok(row)
    }

    pub async fn get_tree(pool: &PgPool, project_id: Option<&str>) -> Result<Option<RaptorTree>> {
        let row = if project_id.is_some() {
            sqlx::query_as::<_, RaptorTree>(
                r#"
                SELECT id, project_id, status, total_nodes, max_depth, created_at, updated_at
                FROM raptor_trees WHERE project_id = $1
                "#,
            )
            .bind(project_id)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as::<_, RaptorTree>(
                r#"
                SELECT id, project_id, status, total_nodes, max_depth, created_at, updated_at
                FROM raptor_trees WHERE project_id IS NULL
                "#,
            )
            .fetch_optional(pool)
            .await?
        };

        Ok(row)
    }

    pub async fn update_tree_stats(
        pool: &PgPool,
        tree_id: &str,
        total_nodes: i64,
        max_depth: i32,
        status: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE raptor_trees
            SET total_nodes = $2, max_depth = $3, status = $4, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(tree_id)
        .bind(total_nodes)
        .bind(max_depth)
        .bind(status)
        .execute(pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_node(
        pool: &PgPool,
        tree_id: &str,
        level: i32,
        parent_id: Option<&str>,
        entity_type: &str,
        entity_id: &str,
        summary: Option<&str>,
        children_count: i32,
    ) -> Result<RaptorNode> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, RaptorNode>(
            r#"
            INSERT INTO raptor_nodes (id, tree_id, level, parent_id, entity_type, entity_id, summary, children_count)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, tree_id, level, parent_id, entity_type, entity_id, summary, children_count, created_at
            "#,
        )
        .bind(&id)
        .bind(tree_id)
        .bind(level)
        .bind(parent_id)
        .bind(entity_type)
        .bind(entity_id)
        .bind(summary)
        .bind(children_count)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Get all nodes for a tree, ordered by level then created_at.
    pub async fn get_collapsed_tree(pool: &PgPool, tree_id: &str) -> Result<Vec<RaptorNode>> {
        let rows = sqlx::query_as::<_, RaptorNode>(
            r#"
            SELECT id, tree_id, level, parent_id, entity_type, entity_id, summary, children_count, created_at
            FROM raptor_nodes
            WHERE tree_id = $1
            ORDER BY level ASC, created_at ASC
            "#,
        )
        .bind(tree_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Delete all nodes for a tree (before rebuilding).
    /// Nullifies parent references first to avoid FK violations on self-referencing rows.
    pub async fn delete_tree_nodes(pool: &PgPool, tree_id: &str) -> Result<()> {
        // Break self-referencing FK links, then delete all nodes at once
        sqlx::query("UPDATE raptor_nodes SET parent_id = NULL WHERE tree_id = $1")
            .bind(tree_id)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM raptor_nodes WHERE tree_id = $1")
            .bind(tree_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn get_tree_by_id(pool: &PgPool, tree_id: &str) -> Result<RaptorTree> {
        let row = sqlx::query_as::<_, RaptorTree>(
            r#"
            SELECT id, project_id, status, total_nodes, max_depth, created_at, updated_at
            FROM raptor_trees WHERE id = $1
            "#,
        )
        .bind(tree_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AlazError::NotFound(format!("raptor tree {tree_id}")))?;

        Ok(row)
    }
}
