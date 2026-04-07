use alaz_core::{AlazError, CircuitBreaker, Result};
use serde::Deserialize;
use tracing::{debug, warn};

/// Service for generating embeddings via Ollama's OpenAI-compatible API.
///
/// Default model: Qwen3-Embedding-8B (4096-dim) for both text and code content,
/// as it ranks #1 on MTEB-Code benchmark. Configurable via TEXT_EMBED_MODEL env var.
pub struct EmbeddingService {
    client: reqwest::Client,
    ollama_url: String,
    text_model: String,
    breaker: CircuitBreaker,
}

#[derive(serde::Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingService {
    /// Create a new embedding service.
    ///
    /// - `ollama_url`: Base URL for Ollama (e.g. "http://localhost:11434")
    /// - `text_model`: Model name for embeddings (e.g. "qwen3-embedding-8b")
    pub fn new(ollama_url: &str, text_model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            ollama_url: ollama_url.trim_end_matches('/').to_string(),
            text_model: text_model.to_string(),
            breaker: CircuitBreaker::new("ollama", 5, 60),
        }
    }

    /// Generate embeddings for content using the text model.
    pub async fn embed_text(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed(&self.text_model, texts).await
    }

    /// Internal embedding call via the Ollama OpenAI-compatible API.
    async fn embed(&self, model: &str, inputs: &[&str]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        if self.breaker.is_open() {
            return Err(AlazError::ServiceUnavailable(
                "ollama circuit breaker open".into(),
            ));
        }

        let url = format!("{}/v1/embeddings", self.ollama_url);
        let request = EmbeddingRequest {
            model,
            input: inputs.to_vec(),
        };

        debug!(
            url = %url,
            model = %model,
            input_count = inputs.len(),
            "sending embedding request"
        );

        let response = match self.client.post(&url).json(&request).send().await {
            Ok(resp) => resp,
            Err(e) => {
                self.breaker.record_failure();
                return Err(AlazError::Embedding(format!("request failed: {e}")));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            warn!(status = %status, body = %body, "embedding API returned error");
            self.breaker.record_failure();
            return Err(AlazError::Embedding(format!(
                "API returned status {status}: {body}"
            )));
        }

        let embed_response: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| AlazError::Embedding(format!("failed to parse response: {e}")))?;

        let embeddings: Vec<Vec<f32>> = embed_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect();

        self.breaker.record_success();

        debug!(
            model = %model,
            count = embeddings.len(),
            dim = embeddings.first().map(|e| e.len()).unwrap_or(0),
            "received embeddings"
        );

        Ok(embeddings)
    }
}
