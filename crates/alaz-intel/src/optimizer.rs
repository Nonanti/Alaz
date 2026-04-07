use std::sync::Arc;

use alaz_core::{Result, estimate_tokens};
use serde::Serialize;
use tracing::debug;

use crate::LlmClient;

/// Optimizes text content to fit within a context window budget.
///
/// 4-stage pipeline:
/// 1. Whitespace cleanup
/// 2. Token estimation
/// 3. LLM summarization (optional, for large sections)
/// 4. Hard truncation (safety net)
pub struct ContextOptimizer {
    llm: Arc<LlmClient>,
}

#[derive(Debug, Serialize)]
pub struct OptimizeResult {
    pub original_tokens: u64,
    pub optimized_tokens: u64,
    pub savings_percent: f64,
    pub optimized_text: String,
    pub sections_summarized: u64,
}

impl ContextOptimizer {
    pub fn new(llm: Arc<LlmClient>) -> Self {
        Self { llm }
    }

    /// Optimize text to fit within a token budget.
    pub async fn optimize(
        &self,
        text: &str,
        max_tokens: u64,
        use_summarization: bool,
    ) -> Result<OptimizeResult> {
        let original_tokens = estimate_tokens(text);

        // Stage 1: Whitespace cleanup
        let cleaned = cleanup_whitespace(text);
        let cleaned_tokens = estimate_tokens(&cleaned);
        debug!(
            original_tokens,
            cleaned_tokens, "stage 1: whitespace cleanup"
        );

        // Stage 2: Check if we're already within budget
        if cleaned_tokens <= max_tokens {
            return Ok(OptimizeResult {
                original_tokens,
                optimized_tokens: cleaned_tokens,
                savings_percent: savings_pct(original_tokens, cleaned_tokens),
                optimized_text: cleaned,
                sections_summarized: 0,
            });
        }

        // Stage 3: LLM summarization for large sections
        let mut summarized = cleaned.clone();
        let mut sections_summarized = 0u64;

        if use_summarization {
            let sections = split_sections(&cleaned);
            let mut output_parts = Vec::with_capacity(sections.len());

            for section in &sections {
                // Only summarize sections larger than ~2000 chars (~500 tokens)
                if section.len() > 2000 {
                    match self.summarize_section(section).await {
                        Ok(summary) => {
                            sections_summarized += 1;
                            output_parts.push(summary);
                        }
                        Err(_) => {
                            // Keep original on LLM failure
                            output_parts.push(section.to_string());
                        }
                    }
                } else {
                    output_parts.push(section.to_string());
                }
            }

            summarized = output_parts.join("\n\n");
        }

        let summarized_tokens = estimate_tokens(&summarized);
        debug!(
            summarized_tokens,
            sections_summarized, "stage 3: summarization"
        );

        // Stage 4: Hard truncation if still over budget
        let max_chars = (max_tokens * 4) as usize;
        let final_text = if summarized.len() > max_chars {
            let mut end = max_chars;
            while end > 0 && !summarized.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\n\n[Truncated for context limits]", &summarized[..end])
        } else {
            summarized
        };

        let optimized_tokens = estimate_tokens(&final_text);

        Ok(OptimizeResult {
            original_tokens,
            optimized_tokens,
            savings_percent: savings_pct(original_tokens, optimized_tokens),
            optimized_text: final_text,
            sections_summarized,
        })
    }

    /// Summarize a single section using LLM.
    async fn summarize_section(&self, section: &str) -> Result<String> {
        let system = "You are a technical text summarizer. Compress the given text while preserving:\n\
            - Code snippets and technical details\n\
            - Key decisions and their rationale\n\
            - Error messages and solutions\n\
            - Important file paths and function names\n\
            Output only the compressed text, no preamble.";

        let prompt = format!(
            "Summarize the following section concisely (target: 30% of original length):\n\n{}",
            section
        );

        self.llm.chat(system, &prompt, 0.3).await
    }
}

/// Replace consecutive whitespace and normalize line breaks.
fn cleanup_whitespace(text: &str) -> String {
    // Collapse horizontal whitespace (spaces/tabs) to single space
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;
    let mut newline_count = 0;

    for ch in text.chars() {
        match ch {
            ' ' | '\t' => {
                if !prev_was_space && newline_count == 0 {
                    result.push(' ');
                    prev_was_space = true;
                }
            }
            '\n' => {
                newline_count += 1;
                prev_was_space = false;
                // Allow max 2 consecutive newlines
                if newline_count <= 2 {
                    result.push('\n');
                }
            }
            '\r' => {} // skip carriage returns
            _ => {
                prev_was_space = false;
                newline_count = 0;
                result.push(ch);
            }
        }
    }

    result
}

/// Split text into logical sections (by double newlines or headers).
fn split_sections(text: &str) -> Vec<&str> {
    let sections: Vec<&str> = text
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
        .collect();

    if sections.is_empty() {
        vec![text]
    } else {
        sections
    }
}

fn savings_pct(original: u64, optimized: u64) -> f64 {
    if original == 0 {
        return 0.0;
    }
    ((original as f64 - optimized as f64) / original as f64 * 100.0).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    // === cleanup_whitespace tests ===

    #[test]
    fn test_cleanup_whitespace() {
        let input = "hello   world\t\ttab\n\n\n\n\nmultiple newlines";
        let result = cleanup_whitespace(input);
        assert_eq!(result, "hello world tab\n\nmultiple newlines");
    }

    #[test]
    fn test_cleanup_whitespace_preserves_single_newline() {
        let input = "line1\nline2\n\nline3";
        let result = cleanup_whitespace(input);
        assert_eq!(result, "line1\nline2\n\nline3");
    }

    #[test]
    fn test_cleanup_whitespace_only_spaces() {
        let result = cleanup_whitespace("     ");
        assert_eq!(result, " ");
    }

    #[test]
    fn test_cleanup_whitespace_only_newlines() {
        let result = cleanup_whitespace("\n\n\n\n\n");
        assert_eq!(result, "\n\n");
    }

    #[test]
    fn test_cleanup_whitespace_mixed_tabs_spaces_newlines() {
        let input = "a \t \t b\n\n\n\nc \t d";
        let result = cleanup_whitespace(input);
        assert_eq!(result, "a b\n\nc d");
    }

    #[test]
    fn test_cleanup_whitespace_empty_string() {
        assert_eq!(cleanup_whitespace(""), "");
    }

    #[test]
    fn test_cleanup_whitespace_carriage_returns_stripped() {
        let input = "line1\r\nline2\r\n";
        let result = cleanup_whitespace(input);
        assert_eq!(result, "line1\nline2\n");
    }

    // === split_sections tests ===

    #[test]
    fn test_split_sections() {
        let input = "section1\n\nsection2\n\nsection3";
        let sections = split_sections(input);
        assert_eq!(sections.len(), 3);
    }

    #[test]
    fn test_split_sections_single_section() {
        let input = "no double newline here just single\nnewlines";
        let sections = split_sections(input);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0], input);
    }

    #[test]
    fn test_split_sections_all_empty() {
        // Multiple double-newlines with only whitespace between them
        let input = "\n\n   \n\n   \n\n";
        let sections = split_sections(input);
        // All sections are whitespace-only, should be filtered out
        // split_sections filters with !s.trim().is_empty()
        // If all empty, returns vec![text]
        assert!(!sections.is_empty());
    }

    #[test]
    fn test_split_sections_whitespace_only_section() {
        let input = "real content\n\n   \n\nmore content";
        let sections = split_sections(input);
        // The whitespace-only section should be filtered out
        assert_eq!(sections.len(), 2);
    }

    // === savings_pct tests ===

    #[test]
    fn test_savings_pct() {
        assert_eq!(savings_pct(100, 50), 50.0);
        assert_eq!(savings_pct(0, 0), 0.0);
    }

    #[test]
    fn test_savings_pct_100_percent() {
        assert_eq!(savings_pct(100, 0), 100.0);
    }

    #[test]
    fn test_savings_pct_0_percent() {
        assert_eq!(savings_pct(100, 100), 0.0);
    }

    #[test]
    fn test_savings_pct_negative() {
        // Optimized is larger than original — negative savings
        let result = savings_pct(100, 150);
        assert!(
            result < 0.0,
            "Should be negative when optimized > original, got {result}"
        );
    }
}
