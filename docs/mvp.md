# Nexo MVP (v0.1)

## Goal

Prove that Nexo can reliably transfer large files between two devices using encrypted peer-to-peer communication.

The MVP is intentionally minimal.

No advanced networking.
No relay network.
No delta engine.
No synchronization.

Only the foundation.

---

## Features

### File Transfer

* Send single files
* Receive single files
* Transfer progress

---

### Encryption

* End-to-end encrypted session
* Ephemeral session keys

---

### Chunking

* Fixed-size chunks
* Chunk verification

---

### Resume Support

* Checkpoint creation
* Resume interrupted transfers

---

### Integrity Verification

* Chunk-level verification
* Full file verification

---

### Parallel Transfers

* Multiple chunk streams
* Configurable stream count

---

## Success Criteria

### Reliability

Transfer a 10 GB file repeatedly without corruption.

### Recovery

Resume successfully after:

* Process restart
* Temporary disconnect
* Network interruption

### Performance

Achieve at least 80% of available bandwidth on a stable connection.

---

## Out of Scope

The following are NOT part of v0.1:

* Mobile applications
* Folder synchronization
* Relay nodes
* Multipath networking
* Delta transfers
* Deduplication
* Mesh networking
* AI features

---

## Deliverable

A desktop application capable of:

Device A

↓

Generate join code

↓

Device B joins

↓

Transfer file

↓

Resume if interrupted

↓

Verify integrity

↓

Complete
