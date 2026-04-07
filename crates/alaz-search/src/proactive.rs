//! Proactive context injection: lightweight FTS-only search for tool-use context.
//!
//! Designed to run within ~50ms, returning max 3 results relevant to the
//! current tool operation (file read, edit, bash command).

use alaz_core::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::debug;

/// A lightweight search result for proactive context injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProactiveResult {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
}

/// Extract search keywords from a tool invocation context.
pub fn extract_keywords(tool: &str, context: &str) -> Option<String> {
    match tool {
        "Read" | "read" => {
            // Extract meaningful path segments from file path
            // e.g. "src/auth/login.rs" -> "auth login"
            extract_path_keywords(context)
        }
        "Edit" | "edit" | "Write" | "write" => {
            // Extract from file path
            extract_path_keywords(context)
        }
        "Bash" | "bash" => {
            // Extract meaningful words from command
            extract_bash_keywords(context)
        }
        _ => None, // Skip other tools
    }
}

/// Extract keywords from a file path.
fn extract_path_keywords(path: &str) -> Option<String> {
    let skip_words = [
        "src", "lib", "crate", "crates", "mod", "index", "main", "test", "tests", "spec", "specs",
    ];

    let keywords: Vec<&str> = path
        .split(&['/', '\\', '.', '-', '_'][..])
        .filter(|s| s.len() > 2 && !skip_words.contains(s))
        .collect();

    if keywords.is_empty() {
        None
    } else {
        // Take at most 4 keywords to keep the query focused
        let kw: Vec<&str> = keywords.into_iter().take(4).collect();
        Some(kw.join(" "))
    }
}

/// Extract keywords from a bash command.
fn extract_bash_keywords(cmd: &str) -> Option<String> {
    let skip_words = [
        "ls", "cd", "cat", "echo", "grep", "find", "rm", "mv", "cp", "mkdir", "git", "cargo",
        "npm", "yarn", "docker", "sudo", "bash", "sh", "zsh", "&&", "||", "|", ">", "<", ">>",
        "2>&1", "--", "-",
    ];

    let keywords: Vec<&str> = cmd
        .split_whitespace()
        .filter(|s| s.len() > 2 && !skip_words.contains(s) && !s.starts_with('-'))
        .take(3)
        .collect();

    if keywords.is_empty() {
        None
    } else {
        Some(keywords.join(" "))
    }
}

/// Perform a lightweight FTS-only search for proactive context.
/// Returns max 3 results, optimized for speed.
pub async fn proactive_search(
    pool: &PgPool,
    keywords: &str,
    project_id: Option<&str>,
) -> Result<Vec<ProactiveResult>> {
    let limit: i64 = 3;

    // Single unified query across all entity types using UNION ALL for efficiency
    let rows = sqlx::query_as::<_, (String, String, String, String, f32)>(
        r#"
        (
            SELECT 'knowledge_item' AS entity_type, id, title,
                   LEFT(content, 150) AS snippet,
                   ts_rank(search_vector, websearch_to_tsquery('simple', $1))::REAL AS rank
            FROM knowledge_items
            WHERE search_vector @@ websearch_to_tsquery('simple', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        UNION ALL
        (
            SELECT 'episode' AS entity_type, id, title,
                   LEFT(content, 150) AS snippet,
                   ts_rank(search_vector, websearch_to_tsquery('simple', $1))::REAL AS rank
            FROM episodes
            WHERE search_vector @@ websearch_to_tsquery('simple', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        UNION ALL
        (
            SELECT 'procedure' AS entity_type, id, title,
                   LEFT(content, 150) AS snippet,
                   ts_rank(search_vector, websearch_to_tsquery('simple', $1))::REAL AS rank
            FROM procedures
            WHERE search_vector @@ websearch_to_tsquery('simple', $1)
              AND ($2::TEXT IS NULL OR project_id = $2)
            ORDER BY rank DESC
            LIMIT $3
        )
        ORDER BY rank DESC
        LIMIT $3
        "#,
    )
    .bind(keywords)
    .bind(project_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let results: Vec<ProactiveResult> = rows
        .into_iter()
        .map(
            |(entity_type, entity_id, title, snippet, score)| ProactiveResult {
                entity_type,
                entity_id,
                title,
                snippet,
                score,
            },
        )
        .collect();

    debug!(
        keywords = %keywords,
        count = results.len(),
        "proactive search complete"
    );

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    // === extract_path_keywords tests ===

    #[test]
    fn test_extract_path_keywords_normal() {
        let result = extract_path_keywords("src/auth/login.rs").unwrap();
        assert!(result.contains("auth"));
        assert!(result.contains("login"));
        // "src" and "rs" (2 chars) should be filtered
        assert!(!result.contains("src"));
    }

    #[test]
    fn test_extract_path_keywords_short_segments_filtered() {
        // All segments <= 2 chars should be filtered
        let result = extract_path_keywords("a/b/c.rs");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_path_keywords_deeply_nested() {
        let result =
            extract_path_keywords("crates/alaz-server/handlers/auth/middleware/jwt.rs").unwrap();
        // Should take at most 4 keywords
        let count = result.split_whitespace().count();
        assert!(count <= 4, "Should have at most 4 keywords, got {count}");
        // Common skip words like "crates" should be filtered
        assert!(!result.contains("crates"));
    }

    #[test]
    fn test_extract_path_keywords_with_hyphens_underscores() {
        let result = extract_path_keywords("my-project/user_settings.rs").unwrap();
        // Splits on '-' and '_'
        assert!(result.contains("project"));
        assert!(result.contains("user"));
        assert!(result.contains("settings"));
    }

    // === extract_bash_keywords tests ===

    #[test]
    fn test_extract_bash_keywords_simple() {
        let result = extract_bash_keywords("cargo test alaz-intel").unwrap();
        // "cargo" is a skip word
        assert!(!result.contains("cargo"));
        assert!(result.contains("test") || result.contains("alaz-intel"));
    }

    #[test]
    fn test_extract_bash_keywords_with_flags() {
        let result = extract_bash_keywords("cargo test --release -p alaz-server");
        // Flags starting with '-' should be filtered
        if let Some(kw) = result {
            assert!(!kw.contains("--release"));
            assert!(!kw.contains("-p"));
        }
    }

    #[test]
    fn test_extract_bash_keywords_all_stop_words() {
        let result = extract_bash_keywords("ls -la");
        // "ls" is a skip word, "-la" starts with '-'
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_bash_keywords_max_three() {
        let result = extract_bash_keywords("echo hello world from rust project code").unwrap();
        let count = result.split_whitespace().count();
        assert!(count <= 3, "Should return at most 3 keywords, got {count}");
    }

    // === extract_keywords routing tests ===

    #[test]
    fn test_extract_keywords_read_tool() {
        let result = extract_keywords("Read", "src/auth/login.rs");
        assert!(result.is_some());
        assert!(result.unwrap().contains("auth"));
    }

    #[test]
    fn test_extract_keywords_edit_tool() {
        let result = extract_keywords("Edit", "crates/alaz-db/repos/knowledge.rs");
        assert!(result.is_some());
    }

    #[test]
    fn test_extract_keywords_bash_tool() {
        let result = extract_keywords("Bash", "cargo test alaz-search");
        assert!(result.is_some());
    }

    #[test]
    fn test_extract_keywords_unknown_tool() {
        let result = extract_keywords("UnknownTool", "some context");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_keywords_write_tool() {
        let result = extract_keywords("Write", "src/handlers/search.rs");
        assert!(result.is_some());
        assert!(result.unwrap().contains("handlers"));
    }
}
