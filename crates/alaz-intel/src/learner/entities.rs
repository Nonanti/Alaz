use alaz_core::models::CreateRelation;
use alaz_db::repos::GraphRepo;
use serde::Deserialize;
use tracing::{debug, warn};

/// A named entity extracted from transcript text.
#[derive(Debug, Deserialize)]
pub(crate) struct ExtractedEntity {
    pub name: String,
    pub entity_type: String,
    pub context: String,
}

const ENTITY_EXTRACTION_PROMPT: &str = r#"Extract named entities from this text. Return a JSON array:
[{"name": "...", "entity_type": "person|technology|file|project|concept", "context": "brief context"}]

Rules:
- Only extract specific, named entities. Skip generic terms.
- entity_type must be one of: "person", "technology", "file", "project", "concept"
- "name" should be the canonical name (e.g., "Rust", not "the Rust language")
- "context" should be a brief phrase explaining how the entity was mentioned
- Extract at most 20 entities
- If there are no notable entities, return an empty array []
- Do NOT extract common programming keywords (e.g., "function", "variable", "loop")
- DO extract specific tools, libraries, frameworks, people, file paths, project names"#;

impl super::SessionLearner {
    /// Extract named entities from transcript text and create graph edges.
    ///
    /// Takes a truncated version of the full transcript (~8KB) and calls the LLM
    /// to identify people, technologies, files, projects, and concepts mentioned.
    /// Each entity becomes a graph edge: `session -> entity_name` with relation "mentions".
    ///
    /// Returns the number of entities successfully saved as graph edges.
    pub(crate) async fn extract_entities(&self, session_id: &str, transcript: &str) -> usize {
        // Truncate to ~8KB for entity extraction
        let max_bytes = 8 * 1024;
        let text = if transcript.len() > max_bytes {
            let mut end = max_bytes;
            while end > 0 && !transcript.is_char_boundary(end) {
                end -= 1;
            }
            &transcript[..end]
        } else {
            transcript
        };

        let entities: Vec<ExtractedEntity> = match self
            .llm
            .chat_json::<Vec<ExtractedEntity>>(ENTITY_EXTRACTION_PROMPT, text, 0.1)
            .await
        {
            Ok(mut entities) => {
                entities.truncate(20);
                entities
            }
            Err(e) => {
                warn!(error = %e, "entity extraction LLM call failed");
                return 0;
            }
        };

        debug!(count = entities.len(), session_id, "extracted entities");

        let mut saved = 0usize;
        for entity in &entities {
            // Validate entity_type
            let valid_types = ["person", "technology", "file", "project", "concept"];
            if !valid_types.contains(&entity.entity_type.as_str()) {
                debug!(
                    name = %entity.name,
                    entity_type = %entity.entity_type,
                    "skipping entity with invalid type"
                );
                continue;
            }

            // Skip empty names
            if entity.name.trim().is_empty() {
                continue;
            }

            let relation = CreateRelation {
                source_type: "session".to_string(),
                source_id: session_id.to_string(),
                target_type: entity.entity_type.clone(),
                target_id: entity.name.clone(),
                relation: "mentions".to_string(),
                weight: Some(1.0),
                description: Some(entity.context.clone()),
                metadata: None,
            };

            match GraphRepo::create_edge(&self.pool, &relation).await {
                Ok(_) => saved += 1,
                Err(e) => {
                    warn!(
                        name = %entity.name,
                        error = %e,
                        "failed to create entity graph edge"
                    );
                }
            }
        }

        debug!(saved, session_id, "entity extraction complete");
        saved
    }
}
