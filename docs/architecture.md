# Nexo Architecture

## Overview

Nexo is designed as a modular system.

Each major responsibility is isolated into its own component to improve maintainability, testing, and scalability.

```text
User Interface
       │
       ▼
Transfer Engine
       │
       ▼
Networking Layer
       │
       ▼
Crypto Layer
       │
       ▼
Storage Layer
```

---

## Core Components

### User Interface

Responsibilities:

* Device discovery
* Transfer management
* Progress reporting
* Settings
* Diagnostics

Future Platforms:

* Desktop
* Mobile
* Web

---

### Transfer Engine

Responsibilities:

* File chunking
* Chunk scheduling
* Transfer orchestration
* Resume handling
* Checkpoint creation
* Integrity verification

Key Principle:

The transfer engine should not care how peers are connected.

---

### Networking Layer

Responsibilities:

* Peer discovery
* Session establishment
* NAT traversal
* Relay fallback
* Multipath connections

Future Features:

* Relay network
* Mesh routing
* Route optimization

---

### Crypto Layer

Responsibilities:

* Key exchange
* Encryption
* Authentication
* Integrity protection

Requirements:

* End-to-end encryption
* Forward secrecy
* Strong modern cryptography

---

### Storage Layer

Responsibilities:

* Temporary chunk storage
* Checkpoint storage
* Resume state management
* Deduplication indexes

Requirements:

* Crash-safe writes
* Fast lookups
* Efficient large-file handling

---

## Future Components

### Delta Engine

Responsibilities:

* Content-defined chunking
* Deduplication
* Delta synchronization

Purpose:

Reduce bandwidth consumption.

---

### Relay Network

Responsibilities:

* Connection fallback
* Difficult NAT traversal
* Reliability improvements

---

### Multipath Controller

Responsibilities:

* Manage multiple connections
* Aggregate throughput
* Automatic failover

Example:

WiFi + Ethernet + VPN

---

## Design Principles

### Reliability First

A slower successful transfer is better than a fast failed transfer.

### Bandwidth Efficiency

Transfer the minimum amount of data possible.

### Modularity

Every subsystem should be independently replaceable.

### Scalability

Architecture should support future desktop, mobile, and distributed deployments.

---

## Architecture Decisions

### MVP Integrity Hashing

For MVP v0.1, SHA-256 chunk and file integrity hashing is implemented in the transfer engine.

Reason:

* Chunk and manifest hashes are part of file chunking, resume validation, and transfer completion.
* This does not include encryption, key exchange, authentication, or session protection.
* End-to-end encryption primitives remain the responsibility of the crypto layer.

This keeps MVP integrity verification close to transfer orchestration while preserving the crypto layer boundary for Phase 5 encryption work.
