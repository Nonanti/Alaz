use alaz_core::{AlazError, CircuitBreaker, Result};
use serde::{Deserialize, de::DeserializeOwned};
use tracing::{debug, warn};

/// Client for LLM inference via Ollama native API or OpenAI-compatible API.
///
/// Detects Ollama by checking if the base URL contains port 11434.
/// Ollama native API supports `think: false` to disable reasoning overhead.
/// Non-Ollama endpoints use the OpenAI-compatible `/chat/completions` format.
pub struct LlmClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    is_ollama: bool,
    breaker: CircuitBreaker,
}

// --- Ollama native API types ---

#[derive(serde::Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    think: bool,
    options: OllamaOptions,
}

#[derive(serde::Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct OllamaOptions {
    temperature: f32,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessageResponse,
}

#[derive(Deserialize)]
struct OllamaMessageResponse {
    content: String,
}

// --- OpenAI-compatible API types ---

#[derive(serde::Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OllamaMessageResponse,
}

impl LlmClient {
    /// Create a new LLM client with a custom base URL.
    ///
    /// Automatically detects Ollama endpoints (port 11434) and uses the
    /// native `/api/chat` endpoint with `think: false` for best performance.
    pub fn with_base_url(api_key: &str, model: &str, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        // Parse port from URL to detect Ollama (more robust than string contains)
        let is_ollama = base
            .split("://")
            .nth(1)
            .and_then(|host_path| host_path.split('/').next())
            .and_then(|host_port| host_port.rsplit(':').next())
            .and_then(|port| port.parse::<u16>().ok())
            == Some(11434);
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: base,
            is_ollama,
            breaker: CircuitBreaker::new("llm", 10, 60),
        }
    }

    /// Send a chat completion request and return the raw text response.
    ///
    /// Uses Ollama native API (`/api/chat` with `think: false`) when detected,
    /// otherwise falls back to OpenAI-compatible `/chat/completions`.
    pub async fn chat(&self, system: &str, user: &str, temperature: f32) -> Result<String> {
        if self.breaker.is_open() {
            return Err(AlazError::ServiceUnavailable(
                "LLM circuit breaker open".into(),
            ));
        }

        let (url, body) = if self.is_ollama {
            self.build_ollama_request(system, user, temperature)
        } else {
            self.build_openai_request(system, user, temperature)
        };

        debug!(url = %url, model = %self.model, ollama = self.is_ollama, "sending chat request");

        // Retry once with 3s backoff on transient failures
        let response = match self.send_request(&url, &body).await {
            Ok(resp) => resp,
            Err(_first_err) => {
                debug!("LLM request failed, retrying in 3s...");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                match self.send_request(&url, &body).await {
                    Ok(resp) => resp,
                    Err(e) => {
                        self.breaker.record_failure();
                        return Err(e);
                    }
                }
            }
        };

        let content = if self.is_ollama {
            let resp: OllamaChatResponse = response
                .json()
                .await
                .map_err(|e| AlazError::Llm(format!("failed to parse Ollama response: {e}")))?;
            resp.message.content
        } else {
            let resp: OpenAIChatResponse = response
                .json()
                .await
                .map_err(|e| AlazError::Llm(format!("failed to parse OpenAI response: {e}")))?;
            resp.choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .ok_or_else(|| AlazError::Llm("empty response from LLM".to_string()))?
        };

        self.breaker.record_success();
        debug!(response_len = content.len(), "received chat response");
        Ok(content)
    }

    /// Send a single HTTP request to the LLM endpoint.
    async fn send_request(&self, url: &str, body: &str) -> Result<reqwest::Response> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .body(body.to_string())
            .header("content-type", "application/json")
            .send()
            .await
            .map_err(|e| AlazError::Llm(format!("request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "LLM API returned error");
            return Err(AlazError::Llm(format!(
                "API returned status {status}: {body}"
            )));
        }

        Ok(response)
    }

    fn build_ollama_request(&self, system: &str, user: &str, temperature: f32) -> (String, String) {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches("/v1"));
        let req = OllamaChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                OllamaChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            stream: false,
            think: false,
            options: OllamaOptions { temperature },
        };
        (url, serde_json::to_string(&req).expect("serialize request"))
    }

    fn build_openai_request(&self, system: &str, user: &str, temperature: f32) -> (String, String) {
        let url = format!("{}/chat/completions", self.base_url);
        let req = OpenAIChatRequest {
            model: self.model.clone(),
            messages: vec![
                OllamaChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                OllamaChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            temperature,
        };
        (url, serde_json::to_string(&req).expect("serialize request"))
    }

    /// Send a chat completion request and parse the response as JSON.
    ///
    /// The LLM response is expected to contain valid JSON (possibly wrapped in
    /// markdown code fences, which are stripped automatically).
    pub async fn chat_json<T: DeserializeOwned>(
        &self,
        system: &str,
        user: &str,
        temperature: f32,
    ) -> Result<T> {
        let raw = self.chat(system, user, temperature).await?;

        // Strip markdown code fences if present
        let json_str = extract_json(&raw);

        serde_json::from_str(json_str).map_err(|e| {
            warn!(
                error = %e,
                raw_len = raw.len(),
                "failed to parse LLM JSON response"
            );
            AlazError::Llm(format!("failed to parse JSON from LLM response: {e}"))
        })
    }
}

/// Extract JSON content from a string that may be wrapped in markdown code fences.
fn extract_json(s: &str) -> &str {
    let trimmed = s.trim();

    // Try to strip ```json ... ``` or ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```json")
        && let Some(json) = rest.strip_suffix("```")
    {
        return json.trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```")
        && let Some(json) = rest.strip_suffix("```")
    {
        return json.trim();
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_fenced() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(extract_json(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_fenced_no_lang() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(extract_json(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_with_whitespace() {
        let input = "  \n  {\"key\": \"value\"}  \n  ";
        assert_eq!(extract_json(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_empty_string() {
        assert_eq!(extract_json(""), "");
    }

    #[test]
    fn test_extract_json_just_fences_no_content() {
        let input = "```json\n```";
        assert_eq!(extract_json(input), "");
    }

    #[test]
    fn test_extract_json_nested_fences() {
        // Edge case: content itself contains ``` but outer fences should be stripped
        let input = "```json\n{\"code\": \"```nested```\"}\n```";
        // strip_prefix/strip_suffix only strips outermost
        let result = extract_json(input);
        assert!(result.contains("nested"));
    }

    #[test]
    fn test_extract_json_no_fences() {
        let input = "[{\"a\": 1}, {\"b\": 2}]";
        assert_eq!(extract_json(input), input);
    }
}
