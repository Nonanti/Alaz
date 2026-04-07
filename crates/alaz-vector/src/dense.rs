use alaz_core::{AlazError, Result};
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, DeletePointsBuilder, Filter, PointId, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder,
};
use tracing::debug;

use crate::client::{COLLECTION_TEXT, point_id};

/// Operations for dense (single-vector) embedding collections.
pub struct DenseVectorOps;

impl DenseVectorOps {
    /// Upsert a text embedding into the `alaz_text` collection.
    pub async fn upsert_text(
        client: &Qdrant,
        entity_type: &str,
        entity_id: &str,
        project_id: Option<&str>,
        embedding: Vec<f32>,
    ) -> Result<()> {
        Self::upsert(
            client,
            COLLECTION_TEXT,
            entity_type,
            entity_id,
            project_id,
            embedding,
        )
        .await
    }

    /// Search the `alaz_text` collection for similar embeddings.
    ///
    /// Returns `(entity_type, entity_id, score)` triples sorted by score descending.
    pub async fn search_text(
        client: &Qdrant,
        embedding: Vec<f32>,
        project: Option<&str>,
        limit: u64,
    ) -> Result<Vec<(String, String, f32)>> {
        Self::search(client, COLLECTION_TEXT, embedding, project, limit).await
    }

    /// Delete a point from the specified collection by (entity_type, entity_id).
    pub async fn delete_point(
        client: &Qdrant,
        collection: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<()> {
        let pid = point_id(entity_type, entity_id);
        let point_id: PointId = pid.into();

        client
            .delete_points(DeletePointsBuilder::new(collection).points(vec![point_id]))
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!(
                    "failed to delete point {entity_type}:{entity_id} from {collection}: {e}"
                ))
            })?;

        debug!(
            collection,
            entity_type, entity_id, "deleted point from qdrant"
        );
        Ok(())
    }

    /// Internal: upsert a single embedding point.
    async fn upsert(
        client: &Qdrant,
        collection: &str,
        entity_type: &str,
        entity_id: &str,
        project_id: Option<&str>,
        embedding: Vec<f32>,
    ) -> Result<()> {
        let pid = point_id(entity_type, entity_id);

        let mut payload = qdrant_client::Payload::new();
        payload.insert("entity_type", entity_type);
        payload.insert("entity_id", entity_id);
        if let Some(project) = project_id {
            payload.insert("project_id", project);
        }

        let point = PointStruct::new(pid, embedding, payload);

        client
            .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(true))
            .await
            .map_err(|e| {
                AlazError::Qdrant(format!(
                    "failed to upsert {entity_type}:{entity_id} into {collection}: {e}"
                ))
            })?;

        debug!(
            collection,
            entity_type, entity_id, "upserted point into qdrant"
        );
        Ok(())
    }

    /// Internal: search a collection for similar embeddings.
    async fn search(
        client: &Qdrant,
        collection: &str,
        embedding: Vec<f32>,
        project: Option<&str>,
        limit: u64,
    ) -> Result<Vec<(String, String, f32)>> {
        let mut builder = SearchPointsBuilder::new(collection, embedding, limit).with_payload(true);

        if let Some(project_id) = project {
            let filter = Filter::must([Condition::matches("project_id", project_id.to_string())]);
            builder = builder.filter(filter);
        }

        let response = client
            .search_points(builder)
            .await
            .map_err(|e| AlazError::Qdrant(format!("failed to search {collection}: {e}")))?;

        let results = response
            .result
            .into_iter()
            .filter_map(|point| {
                let entity_type = point
                    .payload
                    .get("entity_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let entity_id = point
                    .payload
                    .get("entity_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                Some((entity_type, entity_id, point.score))
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_id_deterministic() {
        let id1 = point_id("knowledge_item", "abc123");
        let id2 = point_id("knowledge_item", "abc123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn point_id_different_inputs_differ() {
        let id1 = point_id("knowledge_item", "abc123");
        let id2 = point_id("episode", "abc123");
        assert_ne!(id1, id2);
    }

    #[test]
    fn point_id_shared_across_modules() {
        // Both dense and colbert use the same point_id from client.rs
        let id = point_id("test", "123");
        let id2 = crate::client::point_id("test", "123");
        assert_eq!(id, id2, "point_id should be consistent via client module");
    }
}
