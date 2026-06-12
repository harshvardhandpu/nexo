#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKey(pub Vec<u8>);

pub trait KeyExchange {
    fn public_key(&self) -> &[u8];
    fn complete(&self, peer_public_key: &[u8]) -> std::io::Result<SessionKey>;
}

pub trait Encryptor {
    fn encrypt(&self, plaintext: &[u8]) -> std::io::Result<Vec<u8>>;
    fn decrypt(&self, ciphertext: &[u8]) -> std::io::Result<Vec<u8>>;
}
