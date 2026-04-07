use alaz_core::{AlazError, Result};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

/// Hash a password using Argon2id (memory-hard, side-channel resistant).
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default(); // Argon2id, m=19456, t=2, p=1
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AlazError::Auth(format!("password hashing failed: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed = argon2::PasswordHash::new(hash)
        .map_err(|e| AlazError::Auth(format!("invalid hash format: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let hash = hash_password("my-secret").unwrap();
        assert!(verify_password("my-secret", &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn unique_salts() {
        let h1 = hash_password("same").unwrap();
        let h2 = hash_password("same").unwrap();
        assert_ne!(h1, h2); // Different salts
    }

    #[test]
    fn hash_starts_with_argon2id() {
        let hash = hash_password("test").unwrap();
        assert!(hash.starts_with("$argon2id$"));
    }
}
