use sha2::{Digest, Sha256};
use sqlx::PgPool;

use alaz_core::{AlazError, Result};

/// Compute the SHA-256 hex digest of an API key.
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Look up an API key by its hash, return the owner_id, and update last_used_at.
pub async fn verify_key(pool: &PgPool, key: &str) -> Result<String> {
    let key_hash = hash_key(key);

    let row = sqlx::query_as::<_, (String,)>(
        r#"
        UPDATE api_keys
        SET last_used_at = now()
        WHERE key_hash = $1
        RETURNING owner_id
        "#,
    )
    .bind(&key_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| AlazError::Auth(format!("api key lookup failed: {e}")))?
    .ok_or_else(|| AlazError::Auth("invalid API key".to_string()))?;

    Ok(row.0)
}

/// Encode bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        write!(s, "{b:02x}").expect("write to String is infallible");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_key_deterministic() {
        let h1 = hash_key("my-secret-key");
        let h2 = hash_key("my-secret-key");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn test_hash_key_different_inputs() {
        let h1 = hash_key("key-a");
        let h2 = hash_key("key-b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_empty_string_hash_deterministic() {
        let h1 = hash_key("");
        let h2 = hash_key("");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64, "SHA-256 output must always be 64 hex chars");
    }

    #[test]
    fn test_hash_output_always_64_hex_chars() {
        let inputs = ["", "a", "short", &"x".repeat(100_000)];
        for input in inputs {
            let hash = hash_key(input);
            assert_eq!(
                hash.len(),
                64,
                "hash of {:?}... was {} chars",
                &input[..input.len().min(10)],
                hash.len()
            );
            assert!(
                hash.chars().all(|c| c.is_ascii_hexdigit()),
                "non-hex chars in hash"
            );
        }
    }
}
