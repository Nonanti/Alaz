//! Lightweight code symbol extraction using regex patterns.
//!
//! Extracts functions, structs, enums, traits, impls, consts, and type aliases
//! from Rust source files without a full AST parser. Fast, dependency-free,
//! and sufficient for impact analysis.

use alaz_core::Result;
use alaz_db::repos::{CodeSymbolRepo, UpsertCodeSymbol};
use sqlx::PgPool;
use tracing::{debug, info};

/// A raw extracted symbol before DB storage.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub symbol_type: String,
    pub signature: String,
    pub line_number: i32,
    pub visibility: String,
    pub parent: Option<String>,
    pub deps: Vec<String>,
}

/// Index a single Rust file: extract symbols and store in DB.
///
/// Deletes existing symbols for the file before inserting to handle renames/deletions.
pub async fn index_file(
    pool: &PgPool,
    project_id: Option<&str>,
    file_path: &str,
    content: &str,
) -> Result<usize> {
    let symbols = extract_rust_symbols(content);

    if symbols.is_empty() {
        return Ok(0);
    }

    // Clear old symbols for this file
    CodeSymbolRepo::delete_file(pool, project_id, file_path).await?;

    let mut count = 0;
    for sym in &symbols {
        let input = UpsertCodeSymbol {
            file_path: file_path.to_string(),
            symbol_name: sym.name.clone(),
            symbol_type: sym.symbol_type.clone(),
            signature: Some(sym.signature.clone()),
            line_number: sym.line_number,
            visibility: sym.visibility.clone(),
            parent_symbol: sym.parent.clone(),
            dependencies: sym.deps.clone(),
        };

        if CodeSymbolRepo::upsert(pool, project_id, &input)
            .await
            .is_ok()
        {
            count += 1;
        }
    }

    debug!(file_path, symbols = count, "indexed file");
    Ok(count)
}

/// Index multiple files in batch.
pub async fn index_files(
    pool: &PgPool,
    project_id: Option<&str>,
    files: &[(&str, &str)], // (path, content)
) -> Result<usize> {
    let mut total = 0;
    for (path, content) in files {
        total += index_file(pool, project_id, path, content).await?;
    }
    info!(
        files = files.len(),
        symbols = total,
        "batch indexing complete"
    );
    Ok(total)
}

/// Extract Rust symbols from source code using line-by-line pattern matching.
///
/// # Limitations
///
/// This is a regex-based heuristic, not a full AST parser:
/// - Brace tracking does not account for `{` / `}` inside string literals,
///   comments, or raw strings. This may cause `impl` block scoping to be
///   incorrect for files with complex string content.
/// - Multi-line function signatures are only captured up to the first line.
/// - Macro-generated symbols are not detected.
///
/// These trade-offs are acceptable for impact analysis on typical Rust code.
pub fn extract_rust_symbols(content: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let mut current_impl: Option<String> = None;
    let mut brace_depth: i32 = 0;
    let mut impl_brace_depth: i32 = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_number = (line_idx + 1) as i32;

        // Track brace depth for impl block scoping
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    // If we exit the impl block's brace level, clear current_impl
                    if current_impl.is_some() && brace_depth < impl_brace_depth {
                        current_impl = None;
                    }
                }
                _ => {}
            }
        }

        // Skip comments and attributes
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        let vis = extract_visibility(trimmed);

        // impl Block (track as parent for methods)
        if let Some(impl_name) = parse_impl(trimmed) {
            current_impl = Some(impl_name.clone());
            impl_brace_depth = brace_depth;
            symbols.push(ExtractedSymbol {
                name: impl_name,
                symbol_type: "impl".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis.clone(),
                parent: None,
                deps: extract_deps_from_line(trimmed),
            });
            continue;
        }

        // Function / method
        if let Some(name) = parse_fn(trimmed) {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "function".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: current_impl.clone(),
                deps: extract_deps_from_line(trimmed),
            });
            continue;
        }

        // Struct
        if let Some(name) = parse_keyword(trimmed, "struct") {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "struct".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: None,
                deps: Vec::new(),
            });
            continue;
        }

        // Enum
        if let Some(name) = parse_keyword(trimmed, "enum") {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "enum".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: None,
                deps: Vec::new(),
            });
            continue;
        }

        // Trait
        if let Some(name) = parse_keyword(trimmed, "trait") {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "trait".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: None,
                deps: Vec::new(),
            });
            continue;
        }

        // Const / static
        if let Some(name) = parse_const(trimmed) {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "const".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: current_impl.clone(),
                deps: Vec::new(),
            });
            continue;
        }

        // Type alias
        if let Some(name) = parse_type_alias(trimmed) {
            symbols.push(ExtractedSymbol {
                name,
                symbol_type: "type_alias".to_string(),
                signature: first_line(trimmed),
                line_number,
                visibility: vis,
                parent: None,
                deps: Vec::new(),
            });
        }
    }

    symbols
}

// --- Parsers ---

fn extract_visibility(line: &str) -> String {
    if line.starts_with("pub(crate)") {
        "crate".to_string()
    } else if line.starts_with("pub") {
        "public".to_string()
    } else {
        "private".to_string()
    }
}

/// Parse `fn name` or `async fn name`.
fn parse_fn(line: &str) -> Option<String> {
    let line = strip_qualifiers(line);
    let rest = line.strip_prefix("fn ")?;
    let name = rest.split(['(', '<', ' ']).next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Parse `impl Name` or `impl Trait for Name`.
fn parse_impl(line: &str) -> Option<String> {
    let line = strip_qualifiers(line);
    let rest = line.strip_prefix("impl")?;
    if !rest.starts_with(' ') && !rest.starts_with('<') {
        return None;
    }
    let rest = rest.trim_start();
    // Skip generic params
    let rest = skip_generics(rest);
    let name = rest.split([' ', '{', '<']).next()?;
    if name.is_empty() {
        return None;
    }
    // Handle `impl Trait for Type`
    if let Some(pos) = rest.find(" for ") {
        let after_for = &rest[pos + 5..];
        let type_name = after_for.trim().split([' ', '{', '<']).next()?;
        if !type_name.is_empty() {
            return Some(type_name.to_string());
        }
    }
    Some(name.to_string())
}

/// Parse `struct Name`, `enum Name`, `trait Name`.
fn parse_keyword(line: &str, keyword: &str) -> Option<String> {
    let line = strip_qualifiers(line);
    let prefix = format!("{keyword} ");
    let rest = line.strip_prefix(&prefix)?;
    let name = rest.split([' ', '{', '(', '<', ';', ':']).next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Parse `const NAME` or `static NAME`.
fn parse_const(line: &str) -> Option<String> {
    let line = strip_qualifiers(line);
    let rest = if let Some(r) = line.strip_prefix("const ") {
        r
    } else {
        line.strip_prefix("static ")?
    };
    let name = rest.split([':', ' ']).next()?;
    if name.is_empty() || name == "_" {
        return None;
    }
    Some(name.to_string())
}

/// Parse `type Name = ...`.
fn parse_type_alias(line: &str) -> Option<String> {
    let line = strip_qualifiers(line);
    let rest = line.strip_prefix("type ")?;
    let name = rest.split([' ', '<', '=']).next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Strip `pub`, `pub(crate)`, `async`, `unsafe` prefixes.
fn strip_qualifiers(line: &str) -> &str {
    let mut s = line;
    for prefix in &[
        "pub(crate) ",
        "pub(super) ",
        "pub ",
        "async ",
        "unsafe ",
        "default ",
        "extern \"C\" ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    // Second pass for combinations like `pub async`
    for prefix in &["async ", "unsafe "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    s
}

/// Skip `<...>` generic parameters.
fn skip_generics(s: &str) -> &str {
    if !s.starts_with('<') {
        return s;
    }
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return s[i + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    s
}

/// Extract the first line (up to `{` or end).
fn first_line(s: &str) -> String {
    s.split('{').next().unwrap_or(s).trim().to_string()
}

/// Extract identifiers that look like dependencies from a line.
fn extract_deps_from_line(line: &str) -> Vec<String> {
    let mut deps = Vec::new();
    // Look for `Type::method` patterns
    for word in line.split(|c: char| !c.is_alphanumeric() && c != '_' && c != ':') {
        if let Some(type_name) = word.split("::").next()
            && word.contains("::")
            && !type_name.is_empty()
            && type_name.chars().next().is_some_and(|c| c.is_uppercase())
        {
            deps.push(type_name.to_string());
        }
    }
    deps.sort();
    deps.dedup();
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pub_fn() {
        let symbols = extract_rust_symbols("pub fn hello(name: &str) -> String {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello");
        assert_eq!(symbols[0].symbol_type, "function");
        assert_eq!(symbols[0].visibility, "public");
    }

    #[test]
    fn extract_pub_async_fn() {
        let symbols =
            extract_rust_symbols("    pub async fn search(&self, q: &str) -> Result<Vec<Item>> {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "search");
        assert_eq!(symbols[0].visibility, "public");
    }

    #[test]
    fn extract_private_fn() {
        let symbols = extract_rust_symbols("fn helper(x: i32) -> i32 {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "helper");
        assert_eq!(symbols[0].visibility, "private");
    }

    #[test]
    fn extract_struct() {
        let symbols = extract_rust_symbols("pub struct KnowledgeItem {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "KnowledgeItem");
        assert_eq!(symbols[0].symbol_type, "struct");
    }

    #[test]
    fn extract_enum() {
        let symbols = extract_rust_symbols("pub enum QueryType {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "QueryType");
        assert_eq!(symbols[0].symbol_type, "enum");
    }

    #[test]
    fn extract_trait() {
        let symbols = extract_rust_symbols("pub trait Embeddable: Send + Sync {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Embeddable");
        assert_eq!(symbols[0].symbol_type, "trait");
    }

    #[test]
    fn extract_impl_block() {
        let code = "impl KnowledgeRepo {\n    pub async fn create() {}\n}";
        let symbols = extract_rust_symbols(code);
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "KnowledgeRepo" && s.symbol_type == "impl")
        );
        let method = symbols.iter().find(|s| s.name == "create").unwrap();
        assert_eq!(method.parent.as_deref(), Some("KnowledgeRepo"));
    }

    #[test]
    fn extract_impl_trait_for_type() {
        let symbols = extract_rust_symbols("impl Embeddable for Episode {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Episode");
        assert_eq!(symbols[0].symbol_type, "impl");
    }

    #[test]
    fn extract_const() {
        let symbols = extract_rust_symbols("const MAX_ITEMS: usize = 100;");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "MAX_ITEMS");
        assert_eq!(symbols[0].symbol_type, "const");
    }

    #[test]
    fn extract_pub_crate_visibility() {
        let symbols =
            extract_rust_symbols("pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].visibility, "crate");
    }

    #[test]
    fn extract_type_alias() {
        let symbols =
            extract_rust_symbols("pub type Result<T> = std::result::Result<T, AlazError>;");
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Result");
        assert_eq!(symbols[0].symbol_type, "type_alias");
    }

    #[test]
    fn skip_comments() {
        let code = "// This is a comment\npub fn real() {}";
        let symbols = extract_rust_symbols(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "real");
    }

    #[test]
    fn multiple_symbols() {
        let code = r#"
pub struct Foo {}
pub enum Bar {}
pub fn baz() {}
const QUX: i32 = 42;
"#;
        let symbols = extract_rust_symbols(code);
        assert_eq!(symbols.len(), 4);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"Bar"));
        assert!(names.contains(&"baz"));
        assert!(names.contains(&"QUX"));
    }
}
