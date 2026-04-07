use alaz_core::{AlazError, Result};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::vectors_config::Config as VectorsConfigVariant;
use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use tracing::{info, warn};
use uuid::Uuid;

/// Fixed UUID v5 namespace for generating deterministic point IDs.
pub(crate) const ALAZ_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

/// Generates a deterministic point ID from (entity_type, entity_id) using UUID v5.
pub(crate) fn point_id(entity_type: &str, entity_id: &str) -> String {
    Uuid::new_v5(&ALAZ_NS, format!("{entity_type}:{entity_id}").as_bytes()).to_string()
}

/// Manages the Qdrant vector database connection and collection initialization.
pub struct QdrantManager {
    client: Qdrant,
}

/// Collection name for dense text embeddings.
pub const COLLECTION_TEXT: &str = "alaz_text";

/// Collection name for ColBERT multi-vector embeddings (128-dim).
pub const COLLECTION_COLBERT: &str = "alaz_colbert";

impl QdrantManager {
    /// Connect to Qdrant at the given URL and ensure all required collections exist.
    pub async fn new(url: &str) -> Result<Self> {
        Self::with_text_dim(url, 4096).await
    }

    /// Connect to Qdrant with a custom text embedding dimension.
    pub async fn with_text_dim(url: &str, text_dim: u64) -> Result<Self> {
        let client = Qdrant::from_url(url)
            .build()
            .map_err(|e| AlazError::Qdrant(format!("failed to connect to Qdrant: {e}")))?;

        let manager = Self { client };
        manager.ensure_collections(text_dim).await?;

        Ok(manager)
    }

    /// Returns a reference to the underlying Qdrant client.
    pub fn client(&self) -> &Qdrant {
        &self.client
    }

    /// Ensure all collections exist, creating any that are missing.
    async fn ensure_collections(&self, text_dim: u64) -> Result<()> {
        self.ensure_collection(COLLECTION_TEXT, text_dim, Distance::Cosine)
            .await?;
        self.ensure_collection(COLLECTION_COLBERT, 128, Distance::Cosine)
            .await?;

        Ok(())
    }

    /// Check if a collection exists with the expected dimension.
    /// If it exists with a different dimension, delete and recreate it.
    /// Returns `true` if the collection was recreated (meaning all items need re-embedding).
    pub async fn ensure_collection_dimension(
        &self,
        collection: &str,
        expected_dim: u64,
    ) -> Result<bool> {
        let exists = self
            .client
            .collection_exists(collection)
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!("failed to check collection {collection}: {e}"))
            })?;

        if !exists {
            // Collection doesn't exist yet — create it fresh
            let vectors_config = VectorParamsBuilder::new(expected_dim, Distance::Cosine);
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(collection).vectors_config(vectors_config),
                )
                .await
                .map_err(|e| {
                    AlazError::Qdrant(format!("failed to create collection {collection}: {e}"))
                })?;
            info!(
                collection,
                dimension = expected_dim,
                "created collection (did not exist)"
            );
            return Ok(true);
        }

        // Collection exists — check its current dimension
        let current_dim = self.get_collection_dimension(collection).await?;

        if current_dim == expected_dim {
            info!(
                collection,
                dimension = expected_dim,
                "collection dimension matches"
            );
            return Ok(false);
        }

        // Dimension mismatch — recreate the collection
        warn!(
            collection,
            current_dim,
            expected_dim,
            "collection dimension mismatch — recreating collection (all vectors will be re-embedded)"
        );

        self.client
            .delete_collection(collection)
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!("failed to delete collection {collection}: {e}"))
            })?;

        let vectors_config = VectorParamsBuilder::new(expected_dim, Distance::Cosine);
        self.client
            .create_collection(
                CreateCollectionBuilder::new(collection).vectors_config(vectors_config),
            )
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!("failed to recreate collection {collection}: {e}"))
            })?;

        info!(
            collection,
            old_dim = current_dim,
            new_dim = expected_dim,
            "recreated collection with new dimension"
        );

        Ok(true)
    }

    /// Get the vector dimension of an existing collection.
    async fn get_collection_dimension(&self, collection: &str) -> Result<u64> {
        let info = self.client.collection_info(collection).await.map_err(|e| {
            AlazError::Qdrant(format!(
                "failed to get collection info for {collection}: {e}"
            ))
        })?;

        let config = info
            .result
            .as_ref()
            .and_then(|r| r.config.as_ref())
            .and_then(|c| c.params.as_ref())
            .and_then(|p| p.vectors_config.as_ref())
            .and_then(|vc| vc.config.as_ref());

        match config {
            Some(VectorsConfigVariant::Params(params)) => Ok(params.size),
            Some(VectorsConfigVariant::ParamsMap(map)) => {
                // For named vectors, check the default/first vector
                if let Some(params) = map.map.values().next() {
                    Ok(params.size)
                } else {
                    Err(AlazError::Qdrant(format!(
                        "collection {collection} has empty vector params map"
                    )))
                }
            }
            None => Err(AlazError::Qdrant(format!(
                "collection {collection} has no vector config"
            ))),
        }
    }

    /// Create a single collection if it does not already exist.
    async fn ensure_collection(
        &self,
        name: &str,
        dimension: u64,
        distance: Distance,
    ) -> Result<()> {
        let exists =
            self.client.collection_exists(name).await.map_err(|e| {
                AlazError::Qdrant(format!("failed to check collection {name}: {e}"))
            })?;

        if exists {
            info!(collection = name, "collection already exists");
            return Ok(());
        }

        let vectors_config = VectorParamsBuilder::new(dimension, distance);

        self.client
            .create_collection(CreateCollectionBuilder::new(name).vectors_config(vectors_config))
            .await
            .map_err(|e| AlazError::Qdrant(format!("failed to create collection {name}: {e}")))?;

        info!(collection = name, dimension, "created collection");
        Ok(())
    }
}
