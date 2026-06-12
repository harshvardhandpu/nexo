use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::OsRng;
use sha2::Sha256;
use std::fmt;
use x25519_dalek::{EphemeralSecret, PublicKey};

pub const SESSION_KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKey([u8; SESSION_KEY_LEN]);

impl SessionKey {
    pub fn as_bytes(&self) -> &[u8; SESSION_KEY_LEN] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; SESSION_KEY_LEN]) -> Self {
        Self(bytes)
    }
}

pub struct EphemeralKeyPair {
    secret: EphemeralSecret,
    public: PublicKey,
}

impl EphemeralKeyPair {
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);

        Self { secret, public }
    }

    pub fn public_key(&self) -> PublicKeyBytes {
        PublicKeyBytes(self.public.to_bytes())
    }

    pub fn derive_session_key(
        self,
        peer_public_key: &PublicKeyBytes,
    ) -> Result<SessionKey, CryptoError> {
        let peer_public_key = PublicKey::from(peer_public_key.0);
        let shared_secret = self.secret.diffie_hellman(&peer_public_key);
        derive_session_key(shared_secret.as_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKeyBytes([u8; 32]);

impl PublicKeyBytes {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub trait KeyExchange {
    fn public_key(&self) -> PublicKeyBytes;
    fn complete(self, peer_public_key: &PublicKeyBytes) -> Result<SessionKey, CryptoError>;
}

impl KeyExchange for EphemeralKeyPair {
    fn public_key(&self) -> PublicKeyBytes {
        self.public_key()
    }

    fn complete(self, peer_public_key: &PublicKeyBytes) -> Result<SessionKey, CryptoError> {
        self.derive_session_key(peer_public_key)
    }
}

pub trait Encryptor {
    fn encrypt(
        &self,
        nonce: &[u8],
        associated_data: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    fn decrypt(
        &self,
        nonce: &[u8],
        associated_data: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCipher {
    key: SessionKey,
}

impl SessionCipher {
    pub fn new(key: SessionKey) -> Self {
        Self { key }
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(self.key.as_bytes().into())
    }
}

impl Encryptor for SessionCipher {
    fn encrypt(
        &self,
        nonce: &[u8],
        associated_data: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = validate_nonce(nonce)?;
        self.cipher()
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| CryptoError::EncryptionFailed)
    }

    fn decrypt(
        &self,
        nonce: &[u8],
        associated_data: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = validate_nonce(nonce)?;
        self.cipher()
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| CryptoError::DecryptionFailed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    InvalidNonceLength { expected: usize, actual: usize },
    KeyDerivationFailed,
    EncryptionFailed,
    DecryptionFailed,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::InvalidNonceLength { expected, actual } => {
                write!(
                    formatter,
                    "invalid nonce length: expected {expected} bytes, got {actual}"
                )
            }
            CryptoError::KeyDerivationFailed => {
                formatter.write_str("session key derivation failed")
            }
            CryptoError::EncryptionFailed => formatter.write_str("encryption failed"),
            CryptoError::DecryptionFailed => formatter.write_str("decryption failed"),
        }
    }
}

impl std::error::Error for CryptoError {}

fn derive_session_key(shared_secret: &[u8; 32]) -> Result<SessionKey, CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);
    let mut key = [0u8; SESSION_KEY_LEN];
    hkdf.expand(b"nexo session key v1", &mut key)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;

    Ok(SessionKey(key))
}

fn validate_nonce(nonce: &[u8]) -> Result<&Nonce, CryptoError> {
    if nonce.len() != NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            actual: nonce.len(),
        });
    }

    Ok(Nonce::from_slice(nonce))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x25519_peers_derive_same_session_key() {
        let alice = EphemeralKeyPair::generate();
        let bob = EphemeralKeyPair::generate();
        let alice_public = alice.public_key();
        let bob_public = bob.public_key();

        let alice_key = alice.complete(&bob_public).expect("alice key");
        let bob_key = bob.complete(&alice_public).expect("bob key");

        assert_eq!(alice_key, bob_key);
    }

    #[test]
    fn session_cipher_encrypts_and_decrypts() {
        let key = shared_session_key();
        let cipher = SessionCipher::new(key);
        let nonce = [7u8; NONCE_LEN];
        let aad = b"session-1:transfer-1";
        let plaintext = b"chunk payload";

        let ciphertext = cipher
            .encrypt(&nonce, aad, plaintext)
            .expect("encrypted payload");
        let decrypted = cipher
            .decrypt(&nonce, aad, &ciphertext)
            .expect("decrypted payload");

        assert_ne!(ciphertext, plaintext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_fails_with_wrong_key() {
        let cipher = SessionCipher::new(shared_session_key());
        let wrong_cipher = SessionCipher::new(SessionKey::from_bytes([9u8; SESSION_KEY_LEN]));
        let nonce = [1u8; NONCE_LEN];
        let ciphertext = cipher
            .encrypt(&nonce, b"aad", b"plaintext")
            .expect("ciphertext");

        let error = wrong_cipher
            .decrypt(&nonce, b"aad", &ciphertext)
            .expect_err("wrong key");

        assert_eq!(error, CryptoError::DecryptionFailed);
    }

    #[test]
    fn decrypt_fails_when_ciphertext_is_modified() {
        let cipher = SessionCipher::new(shared_session_key());
        let nonce = [2u8; NONCE_LEN];
        let mut ciphertext = cipher
            .encrypt(&nonce, b"aad", b"plaintext")
            .expect("ciphertext");
        ciphertext[0] ^= 0x01;

        let error = cipher
            .decrypt(&nonce, b"aad", &ciphertext)
            .expect_err("tampered ciphertext");

        assert_eq!(error, CryptoError::DecryptionFailed);
    }

    #[test]
    fn decrypt_fails_when_associated_data_differs() {
        let cipher = SessionCipher::new(shared_session_key());
        let nonce = [3u8; NONCE_LEN];
        let ciphertext = cipher
            .encrypt(&nonce, b"session-a", b"plaintext")
            .expect("ciphertext");

        let error = cipher
            .decrypt(&nonce, b"session-b", &ciphertext)
            .expect_err("wrong aad");

        assert_eq!(error, CryptoError::DecryptionFailed);
    }

    #[test]
    fn invalid_nonce_length_is_rejected() {
        let cipher = SessionCipher::new(shared_session_key());

        let error = cipher
            .encrypt(&[0u8; NONCE_LEN - 1], b"aad", b"plaintext")
            .expect_err("invalid nonce");

        assert_eq!(
            error,
            CryptoError::InvalidNonceLength {
                expected: NONCE_LEN,
                actual: NONCE_LEN - 1,
            }
        );
    }

    fn shared_session_key() -> SessionKey {
        let alice = EphemeralKeyPair::generate();
        let bob = EphemeralKeyPair::generate();
        let bob_public = bob.public_key();

        alice.complete(&bob_public).expect("session key")
    }
}
