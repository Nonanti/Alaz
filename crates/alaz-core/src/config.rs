use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub qdrant_url: String,
    pub ollama_url: String,
    pub tei_url: String,
    pub colbert_url: String,
    pub llm_api_key: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub text_embed_model: String,
    pub text_embed_dim: u64,
    pub jwt_secret: String,
    pub vault_master_key: Option<String>,
    pub listen_addr: String,
    pub cors_origins: Vec<String>,
}

impl AppConfig {
    pub fn from_env() -> crate::Result<Self> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")
                .map_err(|_| crate::AlazError::Validation("DATABASE_URL not set".into()))?,
            qdrant_url: std::env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6333".into()),
            ollama_url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".into()),
            tei_url: std::env::var("TEI_URL").unwrap_or_else(|_| "http://localhost:8001".into()),
            colbert_url: std::env::var("COLBERT_URL")
                .unwrap_or_else(|_| "http://localhost:8002".into()),
            llm_api_key: std::env::var("LLM_API_KEY")
                .or_else(|_| std::env::var("ZHIPUAI_API_KEY"))
                .unwrap_or_default(),
            llm_base_url: std::env::var("LLM_BASE_URL")
                .or_else(|_| std::env::var("ZHIPUAI_BASE_URL"))
                .unwrap_or_else(|_| "http://localhost:11434/v1".into()),
            llm_model: std::env::var("LLM_MODEL")
                .or_else(|_| std::env::var("ZHIPUAI_MODEL"))
                .unwrap_or_else(|_| "qwen3:8b".into()),
            text_embed_model: std::env::var("TEXT_EMBED_MODEL")
                .unwrap_or_else(|_| "qwen3-embedding:8b".into()),
            text_embed_dim: std::env::var("TEXT_EMBED_DIM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4096),
            jwt_secret: {
                let secret = std::env::var("JWT_SECRET")
                    .map_err(|_| crate::AlazError::Validation("JWT_SECRET not set".into()))?;
                if secret.trim().is_empty() {
                    return Err(crate::AlazError::Validation(
                        "JWT_SECRET must not be empty".into(),
                    ));
                }
                secret
            },
            vault_master_key: std::env::var("VAULT_MASTER_KEY").ok(),
            listen_addr: std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3456".into()),
            cors_origins: std::env::var("CORS_ORIGINS")
                .unwrap_or_else(|_| "http://localhost:3456".into())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests need mutual exclusion since they share process-global state
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    unsafe fn set_required_env() {
        unsafe {
            std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/test");
            std::env::set_var("JWT_SECRET", "test-secret");
        }
    }

    unsafe fn clear_required_env() {
        unsafe {
            std::env::remove_var("DATABASE_URL");
            std::env::remove_var("JWT_SECRET");
            std::env::remove_var("LLM_API_KEY");
            std::env::remove_var("ZHIPUAI_API_KEY");
            std::env::remove_var("LLM_BASE_URL");
            std::env::remove_var("ZHIPUAI_BASE_URL");
            std::env::remove_var("LLM_MODEL");
            std::env::remove_var("ZHIPUAI_MODEL");
        }
    }

    #[test]
    fn from_env_missing_database_url_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_required_env();
            std::env::set_var("JWT_SECRET", "test-secret");
        }
        let result = AppConfig::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("DATABASE_URL"), "error: {err}");
        unsafe {
            clear_required_env();
        }
    }

    #[test]
    fn from_env_missing_jwt_secret_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_required_env();
            std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/test");
        }
        let result = AppConfig::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("JWT_SECRET"), "error: {err}");
        unsafe {
            clear_required_env();
        }
    }

    #[test]
    fn from_env_empty_jwt_secret_errors() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_required_env();
            std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/test");
            std::env::set_var("JWT_SECRET", "");
        }
        let result = AppConfig::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("JWT_SECRET"), "error: {err}");
        unsafe {
            clear_required_env();
        }
    }

    #[test]
    fn from_env_defaults_applied() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            clear_required_env();
            set_required_env();
            // Clear optional vars to test defaults
            std::env::remove_var("QDRANT_URL");
            std::env::remove_var("LISTEN_ADDR");
            std::env::remove_var("CORS_ORIGINS");
        }

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.qdrant_url, "http://localhost:6333");
        assert_eq!(config.listen_addr, "0.0.0.0:3456");
        assert!(!config.cors_origins.is_empty());
        assert!(config.vault_master_key.is_none());

        unsafe {
            clear_required_env();
        }
    }
}
