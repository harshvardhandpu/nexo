# Nexo Transfer Protocol v0.1

## Purpose

The Nexo Protocol defines how peers:

* Discover each other
* Authenticate
* Establish encrypted sessions
* Exchange file metadata
* Transfer chunks
* Resume interrupted transfers
* Verify integrity

---

# Protocol Flow

```text
Peer A
  │
  ├── Discovery
  │
  ├── Handshake
  │
  ├── Key Exchange
  │
  ├── Session Established
  │
  ├── Metadata Exchange
  │
  ├── Chunk Transfer
  │
  ├── Integrity Verification
  │
  └── Complete
```

---

# Peer Discovery

## Local Network

Methods:

* mDNS
* Broadcast discovery

Purpose:

Fast LAN discovery without external infrastructure.

---

## Internet Discovery

Methods:

* DHT-based discovery

Future:

* Relay-assisted discovery

---

# Session Establishment

Each transfer receives:

```text
Session ID
```

Properties:

* Random
* Cryptographically secure
* Unique per transfer

---

# Key Exchange

Goals:

* End-to-end encryption
* Forward secrecy

Future candidates:

* X25519
* Noise Protocol Framework

Generated per session.

No long-term transfer keys.

---

# Metadata Exchange

Sender transmits:

```text
File Name
File Size
Chunk Size
File Hash
Transfer ID
```

Receiver validates before accepting.

---

# Chunking

Files are split into chunks.

Initial chunk size:

```text
4 MB
```

Future:

Dynamic chunk sizing based on:

* Network quality
* Storage performance
* Device capability

---

# Chunk Structure

Each chunk contains:

```text
Chunk ID
Offset
Length
Hash
Payload
```

---

# Parallel Streams

Multiple chunks may transfer simultaneously.

Goals:

* Maximize throughput
* Reduce idle time
* Improve large file performance

---

# Resume Support

Receiver stores:

```text
Transfer ID
Received Chunks
Checkpoint Data
```

On reconnect:

Receiver sends:

```text
Missing Chunk List
```

Sender resumes only missing chunks.

---

# Integrity Verification

Chunk Verification:

```text
SHA-256
```

File Verification:

```text
SHA-256
```

Transfer completes only after full verification.

---

# Error Recovery

Supported failures:

* Disconnect
* Sleep
* Application restart
* Temporary network outage

Transfer state must survive interruptions.

---

# Future Extensions

## Delta Engine

Transfer only changed chunks.

---

## Deduplication

Reuse existing chunks on destination.

---

## Multipath Transfers

Use:

* WiFi
* Ethernet
* VPN

simultaneously.

---

## Relay Network

Fallback when direct peer connections fail.

---

# Protocol Principles

1. Reliability over raw speed
2. End-to-end encryption by default
3. Minimize transferred data
4. Resume whenever possible
5. No centralized file storage
