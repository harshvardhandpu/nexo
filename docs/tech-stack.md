# Nexo Technology Stack

## Philosophy

Performance, reliability, and efficiency are prioritized over development speed.

The architecture should support future high-performance networking, large file transfers, and advanced synchronization features.

---

# Core Language

## Rust

Reasons:

* Memory safety
* High performance
* Concurrency support
* Cross-platform support
* Strong networking ecosystem

Rust powers the core transfer engine.

---

# Desktop Application

## Tauri

Reasons:

* Low memory usage
* Native performance
* Small application size
* Strong Rust integration

Frontend communicates with the Rust backend through Tauri commands.

---

# Frontend

## React

Reasons:

* Mature ecosystem
* Strong tooling
* Cross-platform familiarity

---

## TypeScript

Reasons:

* Type safety
* Better maintainability
* Better developer experience

---

# Networking

## QUIC

Reasons:

* Modern transport protocol
* Fast connection establishment
* Stream multiplexing
* Better recovery from packet loss

Potential libraries:

* quinn
* s2n-quic

---

# Encryption

## RustCrypto

Goals:

* End-to-end encryption
* Session key generation
* Integrity verification

Potential algorithms:

* X25519
* ChaCha20-Poly1305
* SHA-256

---

# Serialization

## Protocol Buffers

Reasons:

* Compact
* Fast
* Cross-platform

---

# Storage

## SQLite

Uses:

* Transfer checkpoints
* Session state
* Resume metadata

---

# Future Technologies

## Delta Engine

* FastCDC
* Rabin Fingerprinting

---

## Deduplication

* Content-addressable storage
* Chunk indexes

---

## Relay Network

* Distributed relay nodes
* NAT traversal fallback

---

# Initial Development Target

Linux Desktop

Future:

* Windows
* macOS
* Mobile
