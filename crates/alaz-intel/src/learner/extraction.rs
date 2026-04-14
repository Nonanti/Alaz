use alaz_core::AlazError;
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::domain::ContentDomain;

/// Deserialize any JSON value as a String (handles int, bool, array, null).
/// LLMs sometimes return `60` instead of `"60"` or `["a","b"]` instead of `"a, b"`.
fn flexible_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v: serde_json::Value = Deserialize::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn flexible_string_opt<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v: serde_json::Value = Deserialize::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s),
        other => Some(other.to_string()),
    })
}

fn flexible_string_vec<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v: serde_json::Value = Deserialize::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            })
            .collect(),
        serde_json::Value::Null => vec![],
        serde_json::Value::String(s) => vec![s],
        other => vec![other.to_string()],
    })
}

/// The structured extraction result expected from the LLM.
#[derive(Deserialize)]
pub(crate) struct ExtractionResult {
    #[serde(default)]
    pub patterns: Vec<ExtractedPattern>,
    #[serde(default)]
    pub episodes: Vec<ExtractedEpisode>,
    #[serde(default)]
    pub procedures: Vec<ExtractedProcedure>,
    #[serde(default)]
    pub core_memories: Vec<ExtractedCoreMemory>,
}

#[derive(Deserialize)]
pub(crate) struct ExtractedPattern {
    #[serde(deserialize_with = "flexible_string")]
    pub title: String,
    #[serde(deserialize_with = "flexible_string")]
    pub content: String,
    #[serde(default, deserialize_with = "flexible_string_opt")]
    pub language: Option<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ExtractedEpisode {
    #[serde(deserialize_with = "flexible_string")]
    pub title: String,
    #[serde(deserialize_with = "flexible_string")]
    pub content: String,
    #[serde(default, rename = "type", deserialize_with = "flexible_string_opt")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "flexible_string_opt")]
    pub severity: Option<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub who_cues: Vec<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub what_cues: Vec<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub where_cues: Vec<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub when_cues: Vec<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub why_cues: Vec<String>,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub related_files: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ExtractedProcedure {
    #[serde(deserialize_with = "flexible_string")]
    pub title: String,
    #[serde(deserialize_with = "flexible_string")]
    pub content: String,
    #[serde(default, deserialize_with = "flexible_string_vec")]
    pub steps: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ExtractedCoreMemory {
    #[serde(deserialize_with = "flexible_string")]
    pub category: String,
    #[serde(deserialize_with = "flexible_string")]
    pub key: String,
    #[serde(deserialize_with = "flexible_string")]
    pub value: String,
}

/// Aggregated extraction from all chunks in a session/content.
pub(crate) struct AggregatedExtraction {
    pub patterns: Vec<ExtractedPattern>,
    pub episodes: Vec<ExtractedEpisode>,
    pub procedures: Vec<ExtractedProcedure>,
    pub core_memories: Vec<ExtractedCoreMemory>,
}

const CODING_EXTRACTION_PROMPT: &str = r#"You are a knowledge extraction assistant. Analyze the following session transcript and extract structured knowledge.

Return ONLY valid JSON with this schema:
{
  "patterns": [{"title": "...", "content": "...", "language": "...", "tags": ["..."]}],
  "episodes": [{"title": "...", "content": "...", "type": "error|decision|success|discovery", "severity": "low|medium|high", "who_cues": [], "what_cues": [], "where_cues": [], "when_cues": [], "why_cues": [], "related_files": ["path/to/file.ts", ...]}],
  "procedures": [{"title": "...", "content": "...", "steps": []}],
  "core_memories": [{"category": "preference|fact|convention|constraint", "key": "...", "value": "..."}]
}

Rules:
- Extract at most 3 patterns (reusable code snippets, architectural decisions, design patterns)
- Extract at most 2 episodes (notable events with 5W context cues)
- Extract at most 1 procedure (step-by-step workflows that were followed)
- Extract at most 2 core memories (user preferences, project facts, coding conventions, constraints)
- Be specific and actionable in titles and content
- Only extract genuinely useful knowledge, not trivial observations
- If there is nothing worth extracting, return empty arrays
- Do NOT extract general programming knowledge that any developer would know
- Do NOT extract information about well-known tools/libraries (e.g., "Rust uses cargo")

Core memory key rules:
- Use consistent snake_case keys (e.g., "db_port", "llm_provider", "deploy_method")
- Keep keys short and canonical (max 3 words)
- Reuse existing key names when updating known facts (don't invent new keys for the same concept)
- Example good keys: "db_port", "server_os", "preferred_language", "deploy_command"
- Example bad keys: "Database Port Configuration", "The server operating system"

Episode title rules:
- Start with a verb: "Fixed ...", "Discovered ...", "Decided ...", "Resolved ..."
- Be specific about WHAT happened, not generic descriptions
- Example good: "Fixed tokio::join! borrow error with pre-bound variables"
- Example bad: "Implementation of health check" or "Health Check Endpoint Implementation"

For related_files: list any file paths mentioned in the transcript that relate to this episode. Use exact paths as they appear. Leave empty if no files are mentioned."#;

const PERSONAL_EXTRACTION_PROMPT: &str = r#"You are a personal knowledge extraction assistant. Analyze the following content and extract structured knowledge about the user's life, plans, and experiences.

Return ONLY valid JSON with this schema:
{
  "patterns": [{"title": "...", "content": "...", "language": null, "tags": ["..."]}],
  "episodes": [{"title": "...", "content": "...", "type": "experience|insight|conversation|milestone|observation|recommendation|decision", "severity": "low|medium|high", "who_cues": [], "what_cues": [], "where_cues": [], "when_cues": [], "why_cues": [], "related_files": ["path/to/file", ...]}],
  "procedures": [{"title": "...", "content": "...", "steps": []}],
  "core_memories": [{"category": "preference|fact|convention|constraint", "key": "...", "value": "..."}]
}

Rules:
- Extract at most 2 patterns (recurring habits, routines, preferences)
- Extract at most 3 episodes (experiences, insights, conversations, milestones, observations, recommendations)
- Extract at most 1 procedure (personal routines, workflows)
- Extract at most 3 core memories (personal preferences, facts, life plans, constraints)
- Focus on what matters to the user personally: plans, feelings, relationships, goals
- Capture specific details: names, dates, places, contexts
- If there is nothing worth extracting, return empty arrays

Episode types for personal content:
- "experience" — A personal experience or memory
- "insight" — A lesson learned or realization
- "conversation" — Summary of an important conversation
- "milestone" — Life or project milestone
- "observation" — A notable observation
- "recommendation" — A recommendation (restaurant, book, movie, etc.)
- "decision" — An important personal decision

For related_files: list any file paths mentioned in the transcript that relate to this episode. Use exact paths as they appear. Leave empty if no files are mentioned.

Core memory key rules:
- Use consistent snake_case keys (e.g., "favorite_restaurant", "partner_name", "daily_routine")
- Keep keys short and canonical (max 3 words)"#;

const RESEARCH_EXTRACTION_PROMPT: &str = r#"You are a research knowledge extraction assistant. Analyze the following content and extract structured knowledge about research, learning, and academic topics.

Return ONLY valid JSON with this schema:
{
  "patterns": [{"title": "...", "content": "...", "language": null, "tags": ["..."]}],
  "episodes": [{"title": "...", "content": "...", "type": "discovery|insight|decision", "severity": "low|medium|high", "who_cues": [], "what_cues": [], "where_cues": [], "when_cues": [], "why_cues": [], "related_files": ["path/to/file", ...]}],
  "procedures": [{"title": "...", "content": "...", "steps": []}],
  "core_memories": [{"category": "preference|fact|convention|constraint", "key": "...", "value": "..."}]
}

Rules:
- Extract at most 3 patterns (key concepts, frameworks, theories, methodologies)
- Extract at most 2 episodes (discoveries, insights, important findings)
- Extract at most 1 procedure (research methods, study techniques)
- Extract at most 2 core memories (research interests, key references, methodological preferences)
- Focus on capturing knowledge that can be reused or referenced later
- Include source references where possible
- If there is nothing worth extracting, return empty arrays

For related_files: list any file paths mentioned in the transcript that relate to this episode. Use exact paths as they appear. Leave empty if no files are mentioned.

Core memory key rules:
- Use consistent snake_case keys (e.g., "research_topic", "key_author", "methodology")"#;

const GENERAL_EXTRACTION_PROMPT: &str = r#"You are a knowledge extraction assistant. Analyze the following content and extract structured knowledge.

Return ONLY valid JSON with this schema:
{
  "patterns": [{"title": "...", "content": "...", "language": null, "tags": ["..."]}],
  "episodes": [{"title": "...", "content": "...", "type": "experience|insight|observation|decision|discovery|recommendation", "severity": "low|medium|high", "who_cues": [], "what_cues": [], "where_cues": [], "when_cues": [], "why_cues": [], "related_files": ["path/to/file", ...]}],
  "procedures": [{"title": "...", "content": "...", "steps": []}],
  "core_memories": [{"category": "preference|fact|convention|constraint", "key": "...", "value": "..."}]
}

Rules:
- Extract at most 3 patterns (reusable knowledge, recurring themes)
- Extract at most 2 episodes (notable events, insights, observations)
- Extract at most 1 procedure (step-by-step processes)
- Extract at most 2 core memories (preferences, facts, conventions)
- Be specific and actionable in titles and content
- Only extract genuinely useful knowledge, not trivial observations
- If there is nothing worth extracting, return empty arrays

For related_files: list any file paths mentioned in the transcript that relate to this episode. Use exact paths as they appear. Leave empty if no files are mentioned.

Core memory key rules:
- Use consistent snake_case keys
- Keep keys short and canonical (max 3 words)"#;

/// Select the appropriate extraction prompt for a content domain.
pub(crate) fn extraction_prompt_for_domain(domain: ContentDomain) -> &'static str {
    match domain {
        ContentDomain::Coding => CODING_EXTRACTION_PROMPT,
        ContentDomain::Personal => PERSONAL_EXTRACTION_PROMPT,
        ContentDomain::Research => RESEARCH_EXTRACTION_PROMPT,
        ContentDomain::Health => GENERAL_EXTRACTION_PROMPT,
        ContentDomain::Finance => GENERAL_EXTRACTION_PROMPT,
        ContentDomain::General => GENERAL_EXTRACTION_PROMPT,
    }
}

impl super::SessionLearner {
    /// Extract structured knowledge from content chunks via parallel LLM calls.
    pub(crate) async fn extract_from_chunks(
        &self,
        chunks: &[String],
        domain: ContentDomain,
    ) -> AggregatedExtraction {
        let prompt = extraction_prompt_for_domain(domain);

        let chunk_vec: Vec<(usize, String)> = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.clone()))
            .collect();
        let extraction_results: Vec<(usize, std::result::Result<ExtractionResult, AlazError>)> =
            stream::iter(chunk_vec.into_iter().map(|(i, chunk)| {
                let llm = self.llm.clone();
                async move {
                    debug!(
                        chunk_index = i,
                        chunk_len = chunk.len(),
                        "extracting from chunk"
                    );
                    let result = llm.chat_json::<ExtractionResult>(prompt, &chunk, 0.3).await;
                    (i, result)
                }
            }))
            .buffer_unordered(4)
            .collect()
            .await;

        let mut aggregated = AggregatedExtraction {
            patterns: Vec::new(),
            episodes: Vec::new(),
            procedures: Vec::new(),
            core_memories: Vec::new(),
        };

        for (i, result) in extraction_results {
            match result {
                Ok(extraction) => {
                    aggregated.patterns.extend(extraction.patterns);
                    aggregated.episodes.extend(extraction.episodes);
                    aggregated.procedures.extend(extraction.procedures);
                    aggregated.core_memories.extend(extraction.core_memories);
                }
                Err(e) => {
                    warn!(
                        chunk_index = i,
                        error = %e,
                        "failed to extract from chunk, skipping"
                    );
                }
            }
        }

        // Enforce programmatic caps to guard against over-producing LLMs
        aggregated.patterns.truncate(8);
        aggregated.episodes.truncate(8);
        aggregated.procedures.truncate(4);
        aggregated.core_memories.truncate(8);

        aggregated
    }
}
