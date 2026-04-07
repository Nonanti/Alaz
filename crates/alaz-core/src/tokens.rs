/// Estimate the number of tokens in a text string.
///
/// Uses the industry-standard approximation of ~4 characters per token
/// (ceiling division).
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

/// Truncate a string to at most `max_chars` bytes, respecting UTF-8 character boundaries.
///
/// Returns a slice of the original string up to the largest valid UTF-8 boundary
/// at or before `max_chars` bytes.
pub fn truncate_utf8(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_short_string() {
        // "hi" = 2 chars → (2+3)/4 = 1 token
        assert_eq!(estimate_tokens("hi"), 1);
    }

    #[test]
    fn test_exact_boundary() {
        // 4 chars → (4+3)/4 = 1 token
        assert_eq!(estimate_tokens("abcd"), 1);
        // 5 chars → (5+3)/4 = 2 tokens
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn test_larger_text() {
        let text = "a".repeat(400_000);
        assert_eq!(estimate_tokens(&text), 100_000);
    }

    #[test]
    fn test_truncate_utf8_within_limit() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_utf8_at_ascii_boundary() {
        assert_eq!(truncate_utf8("abcdef", 3), "abc");
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        // 'ü' is 2 bytes
        let s = "aüb";
        let result = truncate_utf8(s, 2);
        assert_eq!(result, "a");
    }

    #[test]
    fn test_truncate_utf8_empty() {
        assert_eq!(truncate_utf8("", 5), "");
    }

    #[test]
    fn test_truncate_utf8_zero_max() {
        assert_eq!(truncate_utf8("hello", 0), "");
    }
}
