//! Markdown-aware content chunking for the Alaz knowledge system.
//!
//! Two chunking strategies:
//!
//! - [`chunk_markdown`]: Structure-aware chunking that respects code blocks,
//!   headers, lists, and paragraphs. Code blocks are never split.
//! - [`chunk_transcript`]: Session-transcript chunking that prefers `[USER]:`
//!   turn boundaries while falling back to hard splitting.
//!
//! Both target ~6 000 tokens per chunk with 200-token overlap between
//! consecutive chunks for context continuity.
//!
//! Token estimation uses [`alaz_core::estimate_tokens`] (char_count / 4).
//! No external tokenizer dependency.

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Target chunk size in estimated tokens (~24 KB chars).
const TARGET_TOKENS: usize = 6_000;
/// Minimum viable chunk size — smaller chunks are merged with neighbours.
const MIN_TOKENS: usize = 500;
/// Maximum chunk size (except code blocks which may exceed this).
const MAX_TOKENS: usize = 8_000;
/// Overlap: tail of previous chunk prepended to next chunk.
const OVERLAP_TOKENS: usize = 200;
/// Characters per estimated token (inverse of `estimate_tokens`).
const CHARS_PER_TOKEN: usize = 4;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Count estimated tokens (re-export from alaz-core).
pub fn estimate_tokens(text: &str) -> u64 {
    alaz_core::estimate_tokens(text)
}

/// Chunk markdown content respecting structure.
///
/// - Code blocks (fenced with ```) are **never** split — kept atomic even
///   when they exceed `MAX_TOKENS`.
/// - Headers (`#`) create natural chunk boundaries.
/// - Lists (`-`, `*`, `1.`) are grouped together.
/// - Paragraphs are kept whole when possible.
/// - Target ≈ 6 000 tokens, min 500, max 8 000.
/// - Consecutive chunks overlap by ~200 tokens for LLM context continuity.
pub fn chunk_markdown(content: &str) -> Vec<String> {
    if content.is_empty() {
        return vec![];
    }
    if toks(content) <= TARGET_TOKENS {
        return vec![content.to_string()];
    }

    let blocks = parse_blocks(content);
    if blocks.is_empty() {
        return vec![content.to_string()];
    }

    let chunks = merge_blocks(blocks);
    let chunks = merge_small(chunks);
    apply_overlap(chunks)
}

/// Chunk a session transcript, preferring `[USER]:` boundaries.
///
/// Splits at `[USER]:` turn markers when possible, merging consecutive
/// turns until the target size is reached. Falls back to hard splitting
/// for runs without markers.
///
/// Target ≈ 6 000 tokens with 200-token overlap.
pub fn chunk_transcript(transcript: &str) -> Vec<String> {
    if transcript.is_empty() {
        return vec![transcript.to_string()];
    }
    if toks(transcript) <= TARGET_TOKENS {
        return vec![transcript.to_string()];
    }

    let marker = "[USER]:";
    let positions: Vec<usize> = transcript
        .match_indices(marker)
        .map(|(pos, _)| pos)
        .collect();

    let raw = if positions.is_empty() {
        hard_split(transcript, TARGET_TOKENS)
    } else {
        // Split into segments at each [USER]: marker.
        let mut segments: Vec<&str> = Vec::new();
        let mut prev = 0;
        for &pos in &positions {
            if pos > prev {
                segments.push(&transcript[prev..pos]);
            }
            prev = pos;
        }
        if prev < transcript.len() {
            segments.push(&transcript[prev..]);
        }

        // Greedily merge segments up to TARGET_TOKENS.
        let mut chunks: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut cur_tok: usize = 0;

        for seg in segments {
            let seg_tok = toks(seg);
            if seg_tok > MAX_TOKENS {
                if !cur.is_empty() {
                    chunks.push(std::mem::take(&mut cur));
                    cur_tok = 0;
                }
                chunks.extend(hard_split(seg, TARGET_TOKENS));
            } else if cur_tok + seg_tok > TARGET_TOKENS && !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
                cur = seg.to_string();
                cur_tok = seg_tok;
            } else {
                cur.push_str(seg);
                cur_tok += seg_tok;
            }
        }
        if !cur.is_empty() {
            chunks.push(cur);
        }
        chunks
    };

    let chunks = merge_small(raw);
    apply_overlap(chunks)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convenience wrapper returning `usize`.
fn toks(text: &str) -> usize {
    alaz_core::estimate_tokens(text) as usize
}

/// Return the tail of `text` corresponding to approximately `n` estimated
/// tokens. UTF-8 safe — always slices on character boundaries.
fn tail_chars(text: &str, n: usize) -> &str {
    let budget = n * CHARS_PER_TOKEN;
    let total = text.chars().count();
    if total <= budget {
        return text;
    }
    let skip = total - budget;
    text.char_indices()
        .nth(skip)
        .map(|(i, _)| &text[i..])
        .unwrap_or(text)
}

/// Hard-split `text` into pieces of at most `max_tokens` estimated tokens.
/// Always splits on UTF-8 character boundaries.
fn hard_split(text: &str, max_tokens: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    let max_chars = max_tokens * CHARS_PER_TOKEN;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut pieces = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let end = (start + max_chars).min(chars.len());
        let start_byte = chars[start].0;
        let end_byte = if end < chars.len() {
            chars[end].0
        } else {
            text.len()
        };
        pieces.push(text[start_byte..end_byte].to_string());
        start = end;
    }

    pieces
}

/// Check if a trimmed line starts a markdown list item.
fn is_list_line(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    // Numbered: `1. `, `12) `, etc.
    let rest = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.len() < trimmed.len() && (rest.starts_with(". ") || rest.starts_with(") "))
}

/// Apply overlap between consecutive chunks.
///
/// For each chunk after the first, prepend the last ~200 estimated tokens
/// of the previous chunk as context.
fn apply_overlap(chunks: Vec<String>) -> Vec<String> {
    if chunks.len() <= 1 {
        return chunks;
    }
    let mut result = Vec::with_capacity(chunks.len());
    result.push(chunks[0].clone());
    for i in 1..chunks.len() {
        let tail = tail_chars(&chunks[i - 1], OVERLAP_TOKENS);
        let mut buf = String::with_capacity(tail.len() + 1 + chunks[i].len());
        buf.push_str(tail);
        buf.push('\n');
        buf.push_str(&chunks[i]);
        result.push(buf);
    }
    result
}

/// Merge chunks smaller than `MIN_TOKENS` with neighbours.
///
/// Forward pass merges small chunks into the previous one. A post-pass
/// handles a still-small first chunk by merging it forward.
fn merge_small(chunks: Vec<String>) -> Vec<String> {
    if chunks.len() <= 1 {
        return chunks;
    }

    let mut result: Vec<String> = Vec::new();
    for chunk in chunks {
        let chunk_tok = toks(&chunk);
        if chunk_tok < MIN_TOKENS
            && let Some(prev) = result.last_mut()
            && toks(prev) + chunk_tok <= MAX_TOKENS
        {
            prev.push_str("\n\n");
            prev.push_str(&chunk);
            continue;
        }
        result.push(chunk);
    }

    // If the first chunk is still tiny, merge it forward.
    if result.len() >= 2 && toks(&result[0]) < MIN_TOKENS {
        let first = result.remove(0);
        let first_tok = toks(&first);
        if first_tok + toks(&result[0]) <= MAX_TOKENS {
            result[0] = format!("{first}\n\n{}", result[0]);
        } else {
            result.insert(0, first);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Markdown block parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum BlockKind {
    Code,
    Header,
    List,
    Paragraph,
}

#[derive(Debug, Clone)]
struct Block {
    kind: BlockKind,
    text: String,
    tokens: usize,
}

/// Parse markdown content into structural blocks.
fn parse_blocks(content: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // --- Fenced code block ---
        if line.trim_start().starts_with("```") {
            let mut buf = vec![line];
            i += 1;
            while i < lines.len() {
                buf.push(lines[i]);
                if lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let text = buf.join("\n");
            blocks.push(Block {
                tokens: toks(&text),
                kind: BlockKind::Code,
                text,
            });
            continue;
        }

        // --- Header ---
        if line.starts_with('#') {
            let text = line.to_string();
            blocks.push(Block {
                tokens: toks(&text),
                kind: BlockKind::Header,
                text,
            });
            i += 1;
            continue;
        }

        // --- List ---
        let trimmed = line.trim_start();
        if is_list_line(trimmed) {
            let mut buf = vec![line];
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let nt = next.trim_start();
                if is_list_line(nt) {
                    buf.push(next);
                    i += 1;
                } else if !nt.is_empty() && (next.starts_with(' ') || next.starts_with('\t')) {
                    // Indented continuation of a list item.
                    buf.push(next);
                    i += 1;
                } else if nt.is_empty()
                    && i + 1 < lines.len()
                    && is_list_line(lines[i + 1].trim_start())
                {
                    // Blank line between list items.
                    buf.push(next);
                    i += 1;
                } else {
                    break;
                }
            }
            let text = buf.join("\n");
            blocks.push(Block {
                tokens: toks(&text),
                kind: BlockKind::List,
                text,
            });
            continue;
        }

        // --- Paragraph ---
        if !trimmed.is_empty() {
            let mut buf = vec![line];
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if next.trim().is_empty()
                    || next.starts_with('#')
                    || next.trim_start().starts_with("```")
                    || is_list_line(next.trim_start())
                {
                    break;
                }
                buf.push(next);
                i += 1;
            }
            let text = buf.join("\n");
            blocks.push(Block {
                tokens: toks(&text),
                kind: BlockKind::Paragraph,
                text,
            });
            continue;
        }

        // Skip empty lines.
        i += 1;
    }

    blocks
}

// ---------------------------------------------------------------------------
// Block → chunk merging
// ---------------------------------------------------------------------------

/// Greedily merge parsed blocks into chunks of approximately
/// `TARGET_TOKENS`.
fn merge_blocks(blocks: Vec<Block>) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_tok: usize = 0;

    for block in blocks {
        match block.kind {
            // Code blocks are never split.  Try fitting in the current chunk
            // first; otherwise emit as a standalone chunk (may exceed MAX).
            BlockKind::Code => {
                if !cur.is_empty() && cur_tok + block.tokens <= MAX_TOKENS {
                    cur.push_str("\n\n");
                    cur.push_str(&block.text);
                    cur_tok += block.tokens;
                } else {
                    if !cur.is_empty() {
                        chunks.push(std::mem::take(&mut cur));
                        cur_tok = 0;
                    }
                    chunks.push(block.text);
                }
            }

            // Headers create natural boundaries — flush the accumulator if
            // it already holds a viable chunk.
            BlockKind::Header => {
                if !cur.is_empty() && cur_tok >= MIN_TOKENS {
                    chunks.push(std::mem::take(&mut cur));
                    cur_tok = 0;
                }
                if !cur.is_empty() {
                    cur.push_str("\n\n");
                }
                cur.push_str(&block.text);
                cur_tok += block.tokens;
            }

            // Lists and paragraphs.
            _ => {
                if cur_tok + block.tokens <= TARGET_TOKENS {
                    if !cur.is_empty() {
                        cur.push_str("\n\n");
                    }
                    cur.push_str(&block.text);
                    cur_tok += block.tokens;
                } else if block.tokens > MAX_TOKENS {
                    // Oversized block — hard-split.  Try to keep the current
                    // accumulator (e.g. a header) attached to the first piece.
                    let pieces = hard_split(&block.text, TARGET_TOKENS);
                    if !cur.is_empty() {
                        let first_tok = toks(&pieces[0]);
                        if cur_tok + first_tok <= MAX_TOKENS {
                            cur.push_str("\n\n");
                            cur.push_str(&pieces[0]);
                            chunks.push(std::mem::take(&mut cur));
                            cur_tok = 0;
                            for piece in pieces.into_iter().skip(1) {
                                chunks.push(piece);
                            }
                        } else {
                            chunks.push(std::mem::take(&mut cur));
                            cur_tok = 0;
                            chunks.extend(pieces);
                        }
                    } else {
                        chunks.extend(pieces);
                    }
                } else {
                    // Block doesn't fit — start a new chunk.
                    if !cur.is_empty() {
                        chunks.push(std::mem::take(&mut cur));
                    }
                    cur = block.text;
                    cur_tok = block.tokens;
                }
            }
        }
    }

    if !cur.is_empty() {
        chunks.push(cur);
    }

    chunks
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Empty input — markdown returns empty vec
    #[test]
    fn test_empty_markdown() {
        assert!(chunk_markdown("").is_empty());
    }

    // 2. Empty input — transcript returns single empty string
    #[test]
    fn test_empty_transcript() {
        let chunks = chunk_transcript("");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    // 3. Small content fits in a single chunk
    #[test]
    fn test_small_content_single_chunk() {
        let text = "# Hello\n\nShort paragraph.\n\n- item 1\n- item 2";
        let chunks = chunk_markdown(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    // 4. Code blocks are never split — even when very large
    #[test]
    fn test_code_block_not_split() {
        // ~110 KB, well above MAX_TOKENS
        let code_body = "let x = 1;\n".repeat(10_000);
        let content = format!("```rust\n{code_body}```");
        let chunks = chunk_markdown(&content);
        let intact = chunks
            .iter()
            .any(|c| c.contains("```rust") && c.ends_with("```"));
        assert!(intact, "Code block must remain intact in one chunk");
    }

    // 5. Headers create chunk boundaries
    #[test]
    fn test_header_boundaries() {
        // Each section body is ~10 000 tokens — forces multiple chunks.
        let body = "Meaningful text with many words. ".repeat(2_000);
        let content = format!("# Section One\n\n{body}\n\n# Section Two\n\n{body}");
        let chunks = chunk_markdown(&content);
        assert!(
            chunks.len() >= 2,
            "Expected ≥2 chunks, got {}",
            chunks.len()
        );
        assert!(chunks[0].contains("# Section One"));
        assert!(
            chunks.iter().any(|c| c.contains("# Section Two")),
            "Some chunk should contain Section Two"
        );
    }

    // 6. Lists are grouped together
    #[test]
    fn test_list_grouped() {
        let list = "- item 1\n- item 2\n- item 3\n- item 4\n- item 5";
        let content = format!("# My List\n\n{list}\n\nSome text after.");
        let chunks = chunk_markdown(&content);
        let has_all = chunks
            .iter()
            .any(|c| c.contains("item 1") && c.contains("item 5"));
        assert!(has_all, "All list items should be grouped together");
    }

    // 7. Overlap is present between consecutive chunks
    #[test]
    fn test_overlap_between_chunks() {
        // Two large sections to guarantee ≥2 chunks.
        let body = "Some meaningful text with unique words. ".repeat(2_000);
        let content = format!("{body}\n\n{body}");
        let chunks = chunk_markdown(&content);
        assert!(chunks.len() >= 2, "Need ≥2 chunks to test overlap");

        // chunks[0] is unchanged by apply_overlap, so
        // tail_chars(chunks[0]) == tail of raw chunk 0.
        let tail = tail_chars(&chunks[0], OVERLAP_TOKENS);
        assert!(
            chunks[1].starts_with(tail),
            "Chunk 2 should start with overlap from chunk 1"
        );
    }

    // 8. Transcript splitting on [USER]: markers
    #[test]
    fn test_transcript_user_markers() {
        let turn = "some long conversation text here. ".repeat(500);
        let transcript = format!(
            "[USER]: first question\n{turn}\n\
             [USER]: second question\n{turn}\n\
             [USER]: third question\n{turn}"
        );
        let chunks = chunk_transcript(&transcript);
        assert!(
            chunks.len() >= 2,
            "Should produce multiple chunks, got {}",
            chunks.len()
        );
        // First chunk should start with the first [USER]: marker.
        assert!(
            chunks[0].starts_with("[USER]:"),
            "First chunk should start with [USER]:"
        );
    }

    // 9. UTF-8 multibyte safety
    #[test]
    fn test_utf8_multibyte() {
        // Turkish characters are multibyte in UTF-8.
        let turkish = "İstanbul'da güzel bir gün. Türkçe karakterler: ğüşıöç. ".repeat(2_000);
        let content = format!("# Türkçe Başlık\n\n{turkish}");
        let chunks = chunk_markdown(&content);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            // Iterating chars would panic on invalid UTF-8.
            let _ = chunk.chars().count();
            assert!(!chunk.is_empty());
        }
    }

    // 10. Very large content produces many chunks
    #[test]
    fn test_very_large_content() {
        let content = "a".repeat(200_000); // ~50 000 tokens
        let chunks = chunk_markdown(&content);
        assert!(
            chunks.len() >= 5,
            "200 KB should produce ≥5 chunks, got {}",
            chunks.len()
        );
    }

    // 11. Mixed content (headers + code + lists + paragraphs)
    #[test]
    fn test_mixed_content() {
        let content = r#"# Introduction

This is the introduction paragraph with enough text to be meaningful.

## Code Example

```python
def hello():
    print("Hello, World!")
    for i in range(100):
        print(i)
```

## List of Features

- Feature one: description
- Feature two: description
- Feature three: description
  - Sub-feature A
  - Sub-feature B

## Conclusion

1. First point
2. Second point
3. Third point
"#;
        let chunks = chunk_markdown(content);
        assert!(!chunks.is_empty());
        // Code block must stay intact.
        let has_code = chunks
            .iter()
            .any(|c| c.contains("def hello()") && c.contains("print(i)"));
        assert!(has_code, "Code block should remain intact");
    }

    // 12. Transcript without [USER]: markers falls back to hard split
    #[test]
    fn test_transcript_no_markers() {
        let content = "x".repeat(TARGET_TOKENS * CHARS_PER_TOKEN * 3);
        let chunks = chunk_transcript(&content);
        assert!(
            chunks.len() >= 3,
            "Should hard-split without markers, got {}",
            chunks.len()
        );
    }

    // 13. estimate_tokens re-export correctness
    #[test]
    fn test_estimate_tokens_reexport() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    // 14. Numbered list detection in parser
    #[test]
    fn test_numbered_list_parsed() {
        let content = "Before.\n\n1. First\n2. Second\n3. Third\n\nAfter.";
        let blocks = parse_blocks(content);
        let list_count = blocks.iter().filter(|b| b.kind == BlockKind::List).count();
        assert_eq!(list_count, 1, "Should detect a numbered list block");
    }

    // 15. Small transcript below target → single chunk, no overlap
    #[test]
    fn test_small_transcript_single_chunk() {
        let text = "[USER]: hello\nSome short reply.";
        let chunks = chunk_transcript(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    // 16. Code block with surrounding prose kept together when small
    #[test]
    fn test_small_code_block_with_prose() {
        let content = "Some intro.\n\n```\ncode\n```\n\nSome outro.";
        let chunks = chunk_markdown(content);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("```\ncode\n```"));
        assert!(chunks[0].contains("Some intro."));
        assert!(chunks[0].contains("Some outro."));
    }

    // 17. hard_split produces valid UTF-8 on multibyte boundaries
    #[test]
    fn test_hard_split_utf8() {
        // Each İ is 2 bytes, so byte-based splitting would break.
        let text = "İ".repeat(100);
        let pieces = hard_split(&text, 10); // 10 tokens = 40 chars
        for piece in &pieces {
            let _ = piece.chars().count(); // panics on invalid UTF-8
        }
        // Total chars should be preserved.
        let total_chars: usize = pieces.iter().map(|p| p.chars().count()).sum();
        assert_eq!(total_chars, 100);
    }

    // 18. tail_chars returns correct tail
    #[test]
    fn test_tail_chars() {
        let text = "abcdefghijklmnop"; // 16 chars
        let tail = tail_chars(text, 2); // 2 tokens = 8 chars
        assert_eq!(tail, "ijklmnop");
    }
}
