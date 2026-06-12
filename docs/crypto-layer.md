# Nexo Crypto Layer

## Purpose

The crypto layer owns cryptographic primitives for secure transfer sessions.

For MVP foundation work, this layer provides:

* Ephemeral key agreement
* Session key derivation
* Authenticated encryption
* Authentication tag verification through AEAD decryption

It does not implement networking, session lifecycle policy, storage, transport retries, Tauri, React, relay nodes, NAT traversal, or peer discovery.

---

## MVP Primitive Choices

MVP encryption uses:

* X25519 for ephemeral key agreement
* HKDF-SHA256 for deriving an encryption key from the shared secret
* ChaCha20-Poly1305 for authenticated encryption

Reason:

* These match the protocol and tech-stack direction for modern end-to-end encryption.
* They keep key exchange and encryption in the crypto crate.
* They avoid coupling the transfer engine to cryptographic implementation details.

---

## Session Key Lifecycle

Each encrypted session should use fresh ephemeral key material.

Expected flow:

```text
Local peer creates ephemeral keypair
Remote peer creates ephemeral keypair
Peers exchange public keys through the session/transport layers
Each peer derives the same session key from X25519 shared secret
Session key encrypts and decrypts transfer messages
```

Long-term identity keys and authenticated pairing are future work.

---

## Encryption Contract

Encryption uses:

* 32-byte session key
* 12-byte nonce
* Optional associated data
* Plaintext input
* Ciphertext output containing the authentication tag

Decryption must fail if:

* The key is wrong
* The nonce is wrong
* Associated data differs
* The ciphertext is modified

Nonce generation and persistence policy are not implemented in this milestone. Callers must provide a unique nonce per encrypted message for a given session key.

---

## Architecture Boundary

The crypto layer reports cryptographic success or failure only.

It does not decide whether a transfer pauses, fails, retries, or cancels. Those decisions belong to the session and engine layers.
