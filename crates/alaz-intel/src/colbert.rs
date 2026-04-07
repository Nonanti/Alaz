use alaz_core::{CircuitBreaker, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Service for generating ColBERT multi-vector embeddings via a Python sidecar
/// serving Jina-ColBERT-v2.
pub struct ColbertService {
    endpoint: String,
    client: reqwest::Client,
    breaker: CircuitBreaker,
}

#[derive(Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
    is_query: bool,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<Vec<f32>>>,
}

impl ColbertService {
    /// Create a new ColBERT service pointing at the sidecar endpoint.
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            breaker: CircuitBreaker::new("colbert", 5, 60),
        }
    }

    /// Generate ColBERT token embeddings for a document.
    /// Returns Vec<Vec<f32>> where each inner vec is a 128-dim token embedding.
    pub async fn embed_document(&self, text: &str) -> Result<Vec<Vec<f32>>> {
        self.embed(text, false).await
    }

    /// Generate ColBERT token embeddings for a query.
    /// Returns Vec<Vec<f32>> where each inner vec is a 128-dim token embedding.
    pub async fn embed_query(&self, query: &str) -> Result<Vec<Vec<f32>>> {
        self.embed(query, true).await
    }

    async fn embed(&self, text: &str, is_query: bool) -> Result<Vec<Vec<f32>>> {
        if self.breaker.is_open() {
            return Ok(vec![]); // Graceful degradation
        }

        let url = format!("{}/embed", self.endpoint);

        let response = match self
            .client
            .post(&url)
            .json(&EmbedRequest {
                texts: vec![text.to_string()],
                is_query,
            })
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                self.breaker.record_failure();
                warn!(error = %e, "ColBERT service unavailable, degrading gracefully");
                return Ok(vec![]);
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            self.breaker.record_failure();
            warn!(status = %status, "ColBERT service error, degrading gracefully");
            return Ok(vec![]);
        }

        let resp: EmbedResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                self.breaker.record_failure();
                warn!(error = %e, "ColBERT response parse error, degrading gracefully");
                return Ok(vec![]);
            }
        };

        self.breaker.record_success();

        let tokens = resp.embeddings.into_iter().next().unwrap_or_default();

        debug!(
            num_tokens = tokens.len(),
            dim = tokens.first().map(|t| t.len()).unwrap_or(0),
            is_query,
            "ColBERT embedding generated"
        );

        Ok(tokens)
    }
}
