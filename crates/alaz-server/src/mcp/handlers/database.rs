use sqlx::Row as _;

use super::super::params::*;
use crate::state::AppState;

/// Execute a read-only SQL query against the database.
/// Only SELECT statements are allowed for safety.
pub(crate) async fn db_query(state: &AppState, params: DbQueryParams) -> Result<String, String> {
    let query = params.query.trim();

    // Security: only allow SELECT statements
    let normalized = query.to_uppercase();
    if !normalized.starts_with("SELECT") && !normalized.starts_with("WITH") {
        return Err("Only SELECT and WITH (CTE) queries are allowed".into());
    }

    // Block dangerous keywords as standalone words (not parts of identifiers)
    let without_strings = strip_sql_strings(&normalized);
    for forbidden in [
        "INSERT ",
        "UPDATE ",
        "DELETE ",
        "DROP ",
        "ALTER ",
        "TRUNCATE ",
        "CREATE ",
        "GRANT ",
        "REVOKE ",
    ] {
        if without_strings.contains(forbidden) {
            let kw = forbidden.trim();
            return Err(format!(
                "Forbidden SQL keyword: {kw}. Only read-only queries allowed."
            ));
        }
    }

    let limit = params.limit.unwrap_or(100).min(500) as i64;

    // Wrap in a limited query if no LIMIT clause present
    let final_query = if !normalized.contains("LIMIT") {
        format!("{query} LIMIT {limit}")
    } else {
        query.to_string()
    };

    let rows = sqlx::query(&final_query)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| format!("Query failed: {e}"))?;

    if rows.is_empty() {
        return Ok("No results.".into());
    }

    // Format results as JSON (most flexible output for arbitrary queries)
    let columns: Vec<String> = {
        use sqlx::Column;
        rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect()
    };

    let mut output = format!("**{} rows returned**\n\n", rows.len());
    output.push_str("| ");
    output.push_str(&columns.join(" | "));
    output.push_str(" |\n|");
    for _ in &columns {
        output.push_str("---|");
    }
    output.push('\n');

    for row in &rows {
        use sqlx::Row;
        output.push_str("| ");
        let vals: Vec<String> = (0..columns.len())
            .map(|i| {
                row.try_get::<String, _>(i)
                    .or_else(|_| row.try_get::<i64, _>(i).map(|v| v.to_string()))
                    .or_else(|_| row.try_get::<i32, _>(i).map(|v| v.to_string()))
                    .or_else(|_| row.try_get::<f64, _>(i).map(|v| format!("{v:.2}")))
                    .or_else(|_| row.try_get::<bool, _>(i).map(|v| v.to_string()))
                    .or_else(|_| {
                        row.try_get::<chrono::DateTime<chrono::Utc>, _>(i)
                            .map(|v| v.format("%Y-%m-%d %H:%M").to_string())
                    })
                    .or_else(|_| {
                        row.try_get::<serde_json::Value, _>(i)
                            .map(|v| v.to_string())
                    })
                    .unwrap_or_else(|_| "NULL".into())
            })
            .collect();
        output.push_str(&vals.join(" | "));
        output.push_str(" |\n");
    }

    Ok(output)
}

/// Explore database schema: list tables, describe columns, show indexes.
pub(crate) async fn db_schema(state: &AppState, params: DbSchemaParams) -> Result<String, String> {
    let action = params.action.as_deref().unwrap_or("tables");

    match action {
        "tables" => list_tables(state).await,
        "describe" => {
            let table = params
                .table
                .as_deref()
                .ok_or("table parameter required for describe")?;
            describe_table(state, table).await
        }
        "indexes" => {
            let table = params
                .table
                .as_deref()
                .ok_or("table parameter required for indexes")?;
            show_indexes(state, table).await
        }
        "fk" => {
            let table = params
                .table
                .as_deref()
                .ok_or("table parameter required for fk")?;
            show_foreign_keys(state, table).await
        }
        _ => Err(format!(
            "Unknown action: {action}. Use: tables, describe, indexes, fk"
        )),
    }
}

async fn list_tables(state: &AppState) -> Result<String, String> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT table_name, pg_size_pretty(pg_total_relation_size(quote_ident(table_name))) as size \
         FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut output = format!("**{} tables**\n\n| Table | Size |\n|---|---|\n", rows.len());
    for (name, size) in &rows {
        output.push_str(&format!("| {name} | {size} |\n"));
    }
    Ok(output)
}

async fn describe_table(state: &AppState, table: &str) -> Result<String, String> {
    // Validate table name (alphanumeric + underscore only)
    if !table.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid table name".into());
    }

    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT column_name, data_type, COALESCE(column_default, '') \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Err(format!("Table '{table}' not found"));
    }

    let mut output = format!(
        "**{table}** ({} columns)\n\n| Column | Type | Default |\n|---|---|---|\n",
        rows.len()
    );
    for (col, dtype, default) in &rows {
        let def = if default.is_empty() {
            "-"
        } else {
            default.as_str()
        };
        output.push_str(&format!("| {col} | {dtype} | {def} |\n"));
    }
    Ok(output)
}

async fn show_indexes(state: &AppState, table: &str) -> Result<String, String> {
    if !table.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid table name".into());
    }

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT indexname, indexdef FROM pg_indexes WHERE tablename = $1 ORDER BY indexname",
    )
    .bind(table)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut output = format!("**Indexes on {table}** ({})\n\n", rows.len());
    for (name, def) in &rows {
        output.push_str(&format!("- **{name}**: `{def}`\n"));
    }
    Ok(output)
}

async fn show_foreign_keys(state: &AppState, table: &str) -> Result<String, String> {
    if !table.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid table name".into());
    }

    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT kcu.column_name, ccu.table_name AS foreign_table, ccu.column_name AS foreign_column \
         FROM information_schema.table_constraints tc \
         JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name \
         JOIN information_schema.constraint_column_usage ccu ON tc.constraint_name = ccu.constraint_name \
         WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = $1",
    )
    .bind(table)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(format!("No foreign keys on {table}"));
    }

    let mut output = format!("**Foreign keys on {table}**\n\n");
    for (col, ftable, fcol) in &rows {
        output.push_str(&format!("- `{col}` → `{ftable}.{fcol}`\n"));
    }
    Ok(output)
}

/// Strip SQL string literals for keyword detection.
fn strip_sql_strings(sql: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut prev = ' ';
    for c in sql.chars() {
        if c == '\'' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            result.push(c);
        }
        prev = c;
    }
    result
}
