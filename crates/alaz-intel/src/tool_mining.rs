use std::collections::HashMap;

use alaz_core::Result;
use tracing::debug;

/// Mines tool usage sequences from session transcripts to discover
/// common tool patterns.
pub struct ToolSequenceMiner;

impl ToolSequenceMiner {
    /// Mine tool sequences from a transcript.
    ///
    /// Parses tool names from the transcript, computes N-grams (3, 4, 5),
    /// and returns sequences that appear 2 or more times.
    ///
    /// Each result is `(tool_sequence, frequency)`.
    pub fn mine(transcript: &str) -> Result<Vec<(Vec<String>, usize)>> {
        // Extract tool names from the transcript
        let tools = extract_tool_names(transcript);

        if tools.len() < 3 {
            debug!(
                tool_count = tools.len(),
                "too few tools for sequence mining"
            );
            return Ok(vec![]);
        }

        let mut all_ngrams: HashMap<Vec<String>, usize> = HashMap::new();

        // Compute N-grams for n = 3, 4, 5
        for n in 3..=5 {
            if tools.len() < n {
                continue;
            }
            for window in tools.windows(n) {
                let key: Vec<String> = window.to_vec();
                *all_ngrams.entry(key).or_insert(0) += 1;
            }
        }

        // Filter to sequences appearing 2+ times
        let mut results: Vec<(Vec<String>, usize)> = all_ngrams
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();

        // Sort by frequency descending, then by sequence length descending
        results.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.len().cmp(&a.0.len())));

        debug!(
            total_tools = tools.len(),
            frequent_sequences = results.len(),
            "tool sequence mining completed"
        );

        Ok(results)
    }
}

/// Extract tool names from a transcript.
///
/// Looks for patterns like:
/// - `[TOOL: tool_name]`
/// - `Tool: tool_name`
/// - `tool_call: tool_name`
/// - Lines starting with tool invocation patterns
fn extract_tool_names(transcript: &str) -> Vec<String> {
    let mut tools = Vec::new();

    for line in transcript.lines() {
        let trimmed = line.trim();

        // Pattern: [TOOL: name] or [Tool: name]
        if let Some(rest) = trimmed
            .strip_prefix("[TOOL:")
            .or_else(|| trimmed.strip_prefix("[Tool:"))
            && let Some(name) = rest.strip_suffix(']')
        {
            let name = name.trim();
            if !name.is_empty() {
                tools.push(name.to_string());
                continue;
            }
        }

        // Pattern: tool_call: name or Tool: name at start of line
        if let Some(rest) = trimmed
            .strip_prefix("tool_call:")
            .or_else(|| trimmed.strip_prefix("Tool:"))
        {
            let name = rest.trim();
            if !name.is_empty() {
                // Take just the first word as the tool name
                let name = name.split_whitespace().next().unwrap_or(name);
                tools.push(name.to_string());
                continue;
            }
        }

        // Pattern: function_call or tool_use markers (common in LLM transcripts)
        if trimmed.contains("function_call") || trimmed.contains("tool_use") {
            // Try to extract the tool name from JSON-like patterns
            if let Some(start) = trimmed.find("\"name\"") {
                let after_name = &trimmed[start + 6..];
                if let Some(colon_pos) = after_name.find(':') {
                    let after_colon = after_name[colon_pos + 1..].trim();
                    let name = after_colon
                        .trim_start_matches('"')
                        .split('"')
                        .next()
                        .unwrap_or("")
                        .trim();
                    if !name.is_empty() {
                        tools.push(name.to_string());
                    }
                }
            }
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_names() {
        let transcript = r#"
[TOOL: read_file]
some output here
[TOOL: edit_file]
more output
[TOOL: run_tests]
test results
[TOOL: read_file]
reading again
[TOOL: edit_file]
editing again
[TOOL: run_tests]
tests pass now
"#;
        let tools = extract_tool_names(transcript);
        assert_eq!(
            tools,
            vec![
                "read_file",
                "edit_file",
                "run_tests",
                "read_file",
                "edit_file",
                "run_tests"
            ]
        );
    }

    #[test]
    fn test_mine_frequent_sequences() {
        let transcript = r#"
[TOOL: read_file]
[TOOL: edit_file]
[TOOL: run_tests]
[TOOL: read_file]
[TOOL: edit_file]
[TOOL: run_tests]
"#;
        let results = ToolSequenceMiner::mine(transcript).unwrap();
        // The sequence [read_file, edit_file, run_tests] should appear 2 times
        assert!(!results.is_empty());
        let seq = results
            .iter()
            .find(|(s, _)| s == &["read_file", "edit_file", "run_tests"]);
        assert!(seq.is_some());
        assert_eq!(seq.unwrap().1, 2);
    }

    #[test]
    fn test_mine_too_few_tools() {
        let transcript = "[TOOL: read_file]\n[TOOL: edit_file]";
        let results = ToolSequenceMiner::mine(transcript).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_function_call_pattern() {
        let transcript = r#"
some text function_call "name": "search_tool" more text
some text function_call "name": "edit_tool" more text
some text function_call "name": "read_tool" more text
some text function_call "name": "search_tool" more text
some text function_call "name": "edit_tool" more text
some text function_call "name": "read_tool" more text
"#;
        let tools = extract_tool_names(transcript);
        assert_eq!(tools.len(), 6);
        assert_eq!(tools[0], "search_tool");
        assert_eq!(tools[1], "edit_tool");
        assert_eq!(tools[2], "read_tool");
    }

    #[test]
    fn test_extract_tool_use_pattern() {
        let transcript = r#"
tool_use "name": "analyze" some output
tool_use "name": "compile" some output
tool_use "name": "test" some output
"#;
        let tools = extract_tool_names(transcript);
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0], "analyze");
        assert_eq!(tools[1], "compile");
        assert_eq!(tools[2], "test");
    }

    #[test]
    fn test_extract_mixed_patterns() {
        let transcript = r#"
[TOOL: read_file]
output here
tool_call: edit_file some args
more output
function_call "name": "run_tests" trailing
"#;
        let tools = extract_tool_names(transcript);
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0], "read_file");
        assert_eq!(tools[1], "edit_file");
        assert_eq!(tools[2], "run_tests");
    }

    #[test]
    fn test_extract_tool_names_with_special_chars() {
        let transcript = "[TOOL: my-special_tool.v2]\noutput\n[TOOL: another/tool]\n";
        let tools = extract_tool_names(transcript);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0], "my-special_tool.v2");
    }

    #[test]
    fn test_mine_no_repeated_sequences() {
        // All unique tools — no sequence repeats
        let transcript = r#"
[TOOL: a]
[TOOL: b]
[TOOL: c]
[TOOL: d]
[TOOL: e]
"#;
        let results = ToolSequenceMiner::mine(transcript).unwrap();
        assert!(results.is_empty(), "No repeated sequences expected");
    }

    #[test]
    fn test_extract_tool_call_colon_pattern() {
        let transcript = "tool_call: deploy\noutput\ntool_call: verify\nok\ntool_call: deploy\n";
        let tools = extract_tool_names(transcript);
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0], "deploy");
        assert_eq!(tools[1], "verify");
        assert_eq!(tools[2], "deploy");
    }
}
