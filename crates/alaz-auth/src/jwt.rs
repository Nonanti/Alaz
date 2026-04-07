use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use alaz_core::{AlazError, Result};

/// JWT claims payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — the owner_id.
    pub sub: String,
    /// Expiration timestamp (seconds since epoch).
    pub exp: i64,
    /// Issued-at timestamp (seconds since epoch).
    pub iat: i64,
}

/// Issue a new HS256-signed JWT token.
pub fn issue_token(owner_id: &str, secret: &str, expiry_hours: i64) -> Result<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: owner_id.to_string(),
        exp: (now + chrono::Duration::hours(expiry_hours)).timestamp(),
        iat: now.timestamp(),
    };

    let token = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AlazError::Auth(format!("failed to issue JWT: {e}")))?;

    Ok(token)
}

/// Verify and decode a JWT token. Returns the Claims if valid.
pub fn verify_token(token: &str, secret: &str) -> Result<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    let data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AlazError::Auth(format!("invalid JWT: {e}")))?;

    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_and_verify() {
        let secret = "test-secret-key-for-testing-only";
        let token = issue_token("user-123", secret, 24).unwrap();
        let claims = verify_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn test_verify_bad_secret() {
        let token = issue_token("user-123", "secret-a", 24).unwrap();
        let result = verify_token(&token, "secret-b");
        assert!(result.is_err());
    }

    #[test]
    fn test_expired_token_rejected() {
        // Issue a token that expired 1 hour ago
        let secret = "test-secret";
        let token = issue_token("user-123", secret, -1);
        // The token should be issued fine (we produce it with exp in the past)
        // but verification should fail
        if let Ok(t) = token {
            let result = verify_token(&t, secret);
            assert!(result.is_err(), "expired token should fail verification");
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("JWT") || err.contains("expired"),
                "error: {err}"
            );
        }
        // If issue itself fails due to negative duration, that's also acceptable
    }

    #[test]
    fn test_empty_secret_works() {
        let token = issue_token("user-123", "", 24).unwrap();
        let claims = verify_token(&token, "").unwrap();
        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn test_very_long_owner_id() {
        let long_id = "u".repeat(10_000);
        let secret = "test-secret";
        let token = issue_token(&long_id, secret, 24).unwrap();
        let claims = verify_token(&token, secret).unwrap();
        assert_eq!(claims.sub, long_id);
    }
}
