use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use alaz_core::{AlazError, Result};
use rand::RngCore;

/// AES-256-GCM encryption for vault secrets.
#[derive(Clone)]
pub struct VaultCrypto {
    cipher: Aes256Gcm,
}

impl VaultCrypto {
    /// Create from a raw 32-byte master key.
    pub fn new(master_key: &[u8; 32]) -> Self {
        let cipher =
            Aes256Gcm::new_from_slice(master_key).expect("32-byte key is always valid for AES-256");
        Self { cipher }
    }

    /// Create from a hex-encoded master key (64 hex chars = 32 bytes).
    pub fn from_hex_key(hex_key: &str) -> Result<Self> {
        let bytes = hex_decode(hex_key)
            .map_err(|e| AlazError::Auth(format!("invalid vault master key hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(AlazError::Auth(format!(
                "vault master key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self::new(&key))
    }

    /// Encrypt plaintext, returning (ciphertext, nonce).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AlazError::Auth(format!("encryption failed: {e}")))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    /// Decrypt ciphertext using the provided nonce.
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        if nonce.len() != 12 {
            return Err(AlazError::Auth(format!(
                "nonce must be 12 bytes, got {}",
                nonce.len()
            )));
        }
        let nonce = Nonce::from_slice(nonce);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| AlazError::Auth(format!("decryption failed: {e}")))
    }
}

/// Decode a hex string into bytes.
fn hex_decode(hex: &str) -> std::result::Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd-length hex string".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("invalid hex at position {i}: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        key
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let crypto = VaultCrypto::new(&test_key());
        let plaintext = b"hello vault secret";
        let (ciphertext, nonce) = crypto.encrypt(plaintext).unwrap();
        let decrypted = crypto.decrypt(&ciphertext, &nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let crypto1 = VaultCrypto::new(&test_key());
        let mut other_key = test_key();
        other_key[0] = 0xFF;
        let crypto2 = VaultCrypto::new(&other_key);

        let (ciphertext, nonce) = crypto1.encrypt(b"secret").unwrap();
        assert!(crypto2.decrypt(&ciphertext, &nonce).is_err());
    }

    #[test]
    fn from_hex_key() {
        let hex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let crypto = VaultCrypto::from_hex_key(hex).unwrap();
        let (ct, nonce) = crypto.encrypt(b"test").unwrap();
        let pt = crypto.decrypt(&ct, &nonce).unwrap();
        assert_eq!(pt, b"test");
    }

    #[test]
    fn bad_hex_key_rejected() {
        assert!(VaultCrypto::from_hex_key("tooshort").is_err());
        assert!(VaultCrypto::from_hex_key("zz").is_err());
    }

    #[test]
    fn encrypt_empty_plaintext() {
        let crypto = VaultCrypto::new(&test_key());
        let (ciphertext, nonce) = crypto.encrypt(b"").unwrap();
        // AES-GCM adds authentication tag even for empty plaintext
        assert!(!ciphertext.is_empty(), "ciphertext should contain auth tag");
        let decrypted = crypto.decrypt(&ciphertext, &nonce).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn wrong_nonce_length_rejected() {
        let crypto = VaultCrypto::new(&test_key());
        let (ciphertext, _nonce) = crypto.encrypt(b"test").unwrap();

        // Too short
        let result = crypto.decrypt(&ciphertext, &[0u8; 8]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("12 bytes"));

        // Too long
        let result = crypto.decrypt(&ciphertext, &[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn large_plaintext_1mb() {
        let crypto = VaultCrypto::new(&test_key());
        let plaintext = vec![0xABu8; 1_000_000]; // 1MB
        let (ciphertext, nonce) = crypto.encrypt(&plaintext).unwrap();
        let decrypted = crypto.decrypt(&ciphertext, &nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
