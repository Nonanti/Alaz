use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An indexed code symbol (function, struct, trait, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CodeSymbol {
    pub id: String,
    pub project_id: Option<String>,
    pub file_path: String,
    pub symbol_name: String,
    pub symbol_type: String,
    pub signature: Option<String>,
    pub line_number: i32,
    pub visibility: String,
    pub parent_symbol: Option<String>,
    pub dependencies: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A symbol with its direct callers/dependents for impact analysis.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolImpact {
    pub symbol: CodeSymbol,
    /// Symbols that directly reference this symbol.
    pub callers: Vec<CodeSymbol>,
}
