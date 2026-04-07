use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlazError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("duplicate: {0}")]
    Duplicate(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("qdrant: {0}")]
    Qdrant(String),
    #[error("embedding: {0}")]
    Embedding(String),
    #[error("llm: {0}")]
    Llm(String),
    #[error("reranker: {0}")]
    Reranker(String),
    #[error("auth: {0}")]
    Auth(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
}

pub type Result<T> = std::result::Result<T, AlazError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_display_format() {
        let err = AlazError::NotFound("item xyz".into());
        assert_eq!(err.to_string(), "not found: item xyz");
    }

    #[test]
    fn duplicate_display_format() {
        let err = AlazError::Duplicate("key abc".into());
        assert_eq!(err.to_string(), "duplicate: key abc");
    }

    #[test]
    fn validation_display_format() {
        let err = AlazError::Validation("bad input".into());
        assert_eq!(err.to_string(), "validation: bad input");
    }

    #[test]
    fn from_sqlx_error() {
        let sqlx_err = sqlx::Error::ColumnNotFound("missing_col".into());
        let err: AlazError = sqlx_err.into();
        assert!(matches!(err, AlazError::Database(_)));
        assert!(err.to_string().contains("missing_col"));
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{bad json").unwrap_err();
        let err: AlazError = json_err.into();
        assert!(matches!(err, AlazError::Json(_)));
        assert!(err.to_string().starts_with("json:"));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: AlazError = io_err.into();
        assert!(matches!(err, AlazError::Io(_)));
        assert!(err.to_string().contains("file missing"));
    }
}
