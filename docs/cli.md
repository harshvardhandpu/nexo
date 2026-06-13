# Nexo CLI MVP

## Purpose

The CLI is the first usable application surface for the Phase 1 foundation.

It connects:

* transfer pipeline chunking, verification, and resume decisions
* storage checkpoints, sessions, and resume metadata
* crypto session primitives for chunk payload encryption
* QUIC localhost transport for real networking
* local peer discovery for LAN visibility

It does not implement GUI, Tauri, NAT traversal, relay networking, delta transfers, folder synchronization, mesh networking, multipath transfer, or production pairing.

---

## Commands

```text
nexo receive
nexo discover
nexo send <file>
nexo send <file> --host <address>
nexo status
```

### nexo discover

Advertises the local CLI peer over mDNS for a short scan window and prints visible Nexo peers on the local network.

The CLI stores a persistent local peer identity in:

```text
peer-id
```

This identity is used only for local discovery. It is not an authentication credential, pairing secret, or transport certificate.

### nexo receive

Starts a QUIC listener on localhost, writes a local receiver advertisement into the CLI state directory, accepts one incoming transfer, writes the received file into the current directory, persists checkpoints as chunks arrive, verifies the completed file, and exits.

The receiver advertisement contains:

* listener socket address
* generated localhost certificate

This is a localhost MVP bootstrap mechanism. It is not peer discovery and is not a production trust model.

### nexo send <file>

Sends one file to the most recent locally advertised receiver.

The sender prepares a `TransferPipelineSender`, opens a QUIC connection, sends the transfer request and chunk metadata, performs an ephemeral key exchange, sends only chunks the receiver reports as missing, waits for file verification, persists local status, and exits.

Chunks are read, verified, encrypted, and sent one at a time. The CLI does not buffer the complete file or all encrypted chunk payloads in memory.

### nexo send <file> --host <address>

Connects to an explicitly supplied socket address.

For the MVP, the address must match a locally stored receiver advertisement so the sender can trust the receiver certificate without adding production pairing or certificate management.

### nexo status

Shows the latest transfer recorded by the CLI state file and storage database.

Status reports:

* transfer ID
* session state when available
* file name
* completed and total chunks
* completed and total bytes

---

## State

The CLI stores local state under:

```text
$NEXO_HOME
```

If `NEXO_HOME` is not set, it uses:

```text
$HOME/.nexo
```

Files:

```text
state.sqlite
receiver.peer
receiver.identity
latest-transfer
peer-id
```

`state.sqlite` is managed through the storage crate. The CLI does not create its own persistence schema.

`receiver.identity` stores the receiver's stable QUIC listening identity: the
self-signed localhost certificate, its private key, and the bound port. It is
written on the first `nexo receive` and reused on every subsequent run so a
restarted receiver keeps the same address and certificate. This is what allows
an interrupted sender to reconnect and resume against a previously advertised
endpoint. The certificate generated here remains a localhost MVP bootstrap
credential, not a production trust anchor.

---

## Transfer Flow

```text
receive
  -> bind QUIC listener
  -> write receiver.peer
  -> accept QUIC connection
  -> accept stream

discover
  -> load or create peer-id
  -> advertise peer over mDNS
  -> collect discovered local peers
  -> print peer display names

send
  -> read receiver.peer
  -> prepare transfer pipeline
  -> connect over QUIC
  -> open stream
  -> send transfer request
  -> send chunk metadata messages

receive
  -> collect request and metadata
  -> load checkpoint
  -> accept transfer through pipeline
  -> send acceptance

send / receive
  -> exchange crypto public keys as control messages
  -> derive session cipher

receive
  -> send missing chunk list from checkpoint

send
  -> send encrypted chunk data for missing chunks

receive
  -> decrypt and verify chunks through pipeline
  -> write chunks
  -> persist checkpoint and resume metadata
  -> verify full file
  -> send file verified

send
  -> acknowledge verification
```

Chunk metadata uses the existing `TransferChunkMessage::Metadata` message. Key exchange uses a transport/session control message carrying an ephemeral public key; cryptographic primitives remain owned by the crypto crate.

---

## Resume Behavior

The receiver checkpoint is authoritative for resume.

When a transfer starts, the receiver loads any checkpoint for the transfer ID. The transfer ID is derived from the file hash so re-sending the same file can resume where possible. A stored checkpoint is reused only when its persisted manifest matches the incoming manifest. Each completed chunk is then read from the destination file and verified against the incoming chunk metadata; missing, truncated, corrupt, duplicate, and unknown checkpoint entries are discarded.

After metadata exchange, the receiver sends the reconciled missing chunk list. The sender skips verified chunks and sends only missing chunks. Both peers update storage-backed progress as chunks are received or sent, and `nexo status` reads the latest persisted session and resume metadata.

The MVP does not implement automatic reconnect within a single transfer. Resume requires running `nexo receive` again and then running `nexo send` again for the same file. Because the receiver now persists its listening identity in `receiver.identity`, a restarted receiver rebinds the same address and presents the same certificate, so a sender that pinned the original `--host` address (or re-reads the rewritten advertisement) can reconnect and continue from the receiver's checkpoint instead of failing to connect.

---

## Security Assumptions

The CLI uses two layers:

* QUIC TLS from the networking crate for transport protection
* crypto crate session primitives for chunk payload encryption

For MVP bootstrapping:

* receiver certificates are self-signed localhost certificates
* sender trust comes from the locally stored receiver advertisement
* discovery identities are informational and do not authenticate a transfer peer
* long-term identity, pairing UX, certificate rotation, and authenticated device trust are future work

The CLI does not weaken the networking crate by adding insecure certificate verification.

---

## Boundaries

The CLI orchestrates existing crates.

It does not:

* hash or verify chunks outside the engine
* persist checkpoints outside the storage crate
* implement QUIC directly
* own cryptographic primitives
* route through relays
* decide future reconnect policy
