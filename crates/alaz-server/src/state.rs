use std::sync::Arc;

use alaz_auth::VaultCrypto;
use alaz_core::{AppConfig, Result};
use alaz_db::create_pool;
use alaz_intel::HydeGenerator;
use alaz_intel::{ColbertService, EmbeddingService, LlmClient};
use alaz_search::{Reranker, SearchCache, SearchPipeline};
use alaz_vector::{COLLECTION_TEXT, QdrantManager};
use sqlx::PgPool;
use tracing::{info, warn};

/// Shared application state for both REST and MCP endpoints.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub qdrant: Arc<QdrantManager>,
    pub llm: Arc<LlmClient>,
    pub embedding: Arc<EmbeddingService>,
    pub colbert: Arc<ColbertService>,
    pub search: Arc<SearchPipeline>,
    pub vault_crypto: Option<VaultCrypto>,
    pub config: Arc<AppConfig>,
    pub metrics: crate::SharedMetrics,
}

impl AppState {
    /// Initialize all components from the application configuration.
    pub async fn new(config: AppConfig) -> Result<Self> {
        let pool = create_pool(&config.database_url).await?;

        info!(
            model = %config.text_embed_model,
            dim = config.text_embed_dim,
            "initializing embedding configuration"
        );

        let qdrant = Arc::new(
            QdrantManager::with_text_dim(&config.qdrant_url, config.text_embed_dim).await?,
        );

        // Check if the text collection dimension matches the configured dimension.
        // If there's a mismatch (e.g. upgrading from 1024-dim to 4096-dim), the
        // collection will be recreated and all items flagged for re-embedding.
        let recreated = qdrant
            .ensure_collection_dimension(COLLECTION_TEXT, config.text_embed_dim)
            .await?;

        if recreated {
            warn!(
                dim = config.text_embed_dim,
                "text collection was recreated — resetting all embeddings for re-processing"
            );
            alaz_db::reset_all_embeddings(&pool).await?;
        }

        let llm = Arc::new(LlmClient::with_base_url(
            &config.llm_api_key,
            &config.llm_model,
            &config.llm_base_url,
        ));
        let embedding = Arc::new(EmbeddingService::new(
            &config.ollama_url,
            &config.text_embed_model,
        ));
        let colbert = Arc::new(ColbertService::new(&config.colbert_url));

        let reranker = Reranker::new(&config.tei_url, Some(llm.clone()));
        let hyde = HydeGenerator::new(llm.clone());

        let search = Arc::new(SearchPipeline {
            pool: pool.clone(),
            qdrant: qdrant.clone(),
            embedding: embedding.clone(),
            colbert: colbert.clone(),
            reranker,
            hyde,
            cache: SearchCache::new(60, 100),
        });

        let vault_crypto = config
            .vault_master_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .map(VaultCrypto::from_hex_key)
            .transpose()?;

        Ok(Self {
            pool,
            qdrant,
            llm,
            embedding,
            colbert,
            search,
            vault_crypto,
            config: Arc::new(config),
            metrics: Arc::new(crate::Metrics::new()),
        })
    }
}
