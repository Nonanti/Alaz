use alaz_core::Result;
use alaz_core::models::CodeSymbol;
use sqlx::PgPool;

/// Input for upserting a code symbol.
pub struct UpsertCodeSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub signature: Option<String>,
    pub line_number: i32,
    pub visibility: String,
    pub parent_symbol: Option<String>,
    pub dependencies: Vec<String>,
}

pub struct CodeSymbolRepo;

impl CodeSymbolRepo {
    /// Upsert a code symbol (insert or update on conflict).
    pub async fn upsert(
        pool: &PgPool,
        project_id: Option<&str>,
        input: &UpsertCodeSymbol,
    ) -> Result<CodeSymbol> {
        let id = cuid2::create_id();

        let row = sqlx::query_as::<_, CodeSymbol>(
            r#"
            INSERT INTO code_symbols
                (id, project_id, file_path, symbol_name, symbol_type, signature,
                 line_number, visibility, parent_symbol, dependencies)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (project_id, file_path, symbol_name, symbol_type) DO UPDATE SET
                signature = EXCLUDED.signature,
                line_number = EXCLUDED.line_number,
                visibility = EXCLUDED.visibility,
                parent_symbol = EXCLUDED.parent_symbol,
                dependencies = EXCLUDED.dependencies,
                updated_at = now()
            RETURNING id, project_id, file_path, symbol_name, symbol_type, signature,
                      line_number, visibility, parent_symbol, dependencies,
                      created_at, updated_at
            "#,
        )
        .bind(&id)
        .bind(project_id)
        .bind(&input.file_path)
        .bind(&input.symbol_name)
        .bind(&input.symbol_type)
        .bind(&input.signature)
        .bind(input.line_number)
        .bind(&input.visibility)
        .bind(&input.parent_symbol)
        .bind(&input.dependencies)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Find all symbols that reference the given symbol name in their dependencies.
    pub async fn find_callers(
        pool: &PgPool,
        project_id: Option<&str>,
        symbol_name: &str,
    ) -> Result<Vec<CodeSymbol>> {
        let rows = sqlx::query_as::<_, CodeSymbol>(
            r#"
            SELECT id, project_id, file_path, symbol_name, symbol_type, signature,
                   line_number, visibility, parent_symbol, dependencies,
                   created_at, updated_at
            FROM code_symbols
            WHERE $1 = ANY(dependencies)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY file_path, line_number
            "#,
        )
        .bind(symbol_name)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get a symbol by name (optionally scoped to project and parent).
    pub async fn get_by_name(
        pool: &PgPool,
        project_id: Option<&str>,
        symbol_name: &str,
    ) -> Result<Vec<CodeSymbol>> {
        let rows = sqlx::query_as::<_, CodeSymbol>(
            r#"
            SELECT id, project_id, file_path, symbol_name, symbol_type, signature,
                   line_number, visibility, parent_symbol, dependencies,
                   created_at, updated_at
            FROM code_symbols
            WHERE symbol_name = $1
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY file_path, line_number
            "#,
        )
        .bind(symbol_name)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List all symbols in a file.
    pub async fn list_by_file(
        pool: &PgPool,
        project_id: Option<&str>,
        file_path: &str,
    ) -> Result<Vec<CodeSymbol>> {
        let rows = sqlx::query_as::<_, CodeSymbol>(
            r#"
            SELECT id, project_id, file_path, symbol_name, symbol_type, signature,
                   line_number, visibility, parent_symbol, dependencies,
                   created_at, updated_at
            FROM code_symbols
            WHERE file_path = $1
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY line_number
            "#,
        )
        .bind(file_path)
        .bind(project_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Delete all symbols for a file (before re-indexing).
    pub async fn delete_file(
        pool: &PgPool,
        project_id: Option<&str>,
        file_path: &str,
    ) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM code_symbols
            WHERE file_path = $1
              AND ($2::TEXT IS NULL OR project_id = $2)
            "#,
        )
        .bind(file_path)
        .bind(project_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}
