# Nexo Transfer Pipeline MVP

## Purpose

The transfer pipeline MVP connects the existing Phase 1 foundation pieces into the first complete end-to-end file transfer workflow.

It uses:

* engine chunking and verification
* session lifecycle models
* loopback transport
* storage checkpoints and resume metadata
* crypto session primitives through an engine-owned cipher boundary

It does not implement QUIC, sockets, NAT traversal, relay nodes, UI, synchronization, delta transfers, deduplication, or production deployment.

---

## Transfer Lifecycle

```text
Prepare
  -> Create session
  -> Exchange transfer request
  -> Accept transfer
  -> Send chunks
  -> Verify chunks
  -> Persist checkpoints
  -> Verify full file
  -> Complete session
```

The sender generates a manifest and chunk metadata from the source file. The receiver accepts the transfer request, receives chunks over a transport stream, verifies each chunk, writes each verified chunk to the destination file, and updates checkpoint state.

The transfer completes only after the reconstructed file hash matches the manifest hash.

---

## Sender Workflow

The sender:

1. Builds a `TransferPipelineConfig`.
2. Generates a `FileManifest`.
3. Splits the file into fixed-size chunk metadata.
4. Creates a `TransferRequest`.
5. Sends the request as a session message.
6. Reads missing chunks from disk.
7. Verifies each source chunk before sending.
8. Encrypts chunk payload bytes through the pipeline cipher trait.
9. Sends chunk data messages.
10. Receives final verification from the receiver.

The sender uses the receiver checkpoint to skip chunks that are already complete during resume.

---

## Receiver Workflow

The receiver:

1. Starts with a session and checkpoint.
2. Receives a transfer request.
3. Validates that the request matches the expected session and peers.
4. Accepts the transfer.
5. Receives chunk data messages.
6. Decrypts payload bytes through the pipeline cipher trait.
7. Verifies each chunk against sender metadata.
8. Writes verified chunks at their file offsets.
9. Updates checkpoint state.
10. Verifies the reconstructed file hash.
11. Transitions the session to completed.

The receiver persists checkpoint and resume metadata through the storage crate, not through the networking layer.

---

## Chunk Flow

```text
source file
    |
    v
chunk metadata + plaintext chunk
    |
    v
chunk verification
    |
    v
payload encryption boundary
    |
    v
MessageEnvelope::Chunk(Data)
    |
    v
loopback stream
    |
    v
payload decryption boundary
    |
    v
chunk verification
    |
    v
write at destination offset
```

The loopback transport moves `MessageEnvelope` values only. It does not know whether payloads are encrypted, verified, accepted, or persisted.

---

## Verification Flow

Chunk verification uses SHA-256 metadata produced by the engine.

For every received chunk:

* chunk ID must match
* offset must match
* size must match
* payload hash must match metadata hash

Full-file verification hashes the reconstructed destination file and compares it to the sender manifest hash.

If full-file verification succeeds, the receiver emits a `FileVerified` message and marks the session complete.

---

## Checkpoint Flow

The receiver checkpoint stores completed chunk IDs for a transfer.

After each verified chunk:

1. The receiver marks the chunk complete.
2. The storage layer persists the checkpoint.
3. The storage layer persists resume metadata with the manifest and latest checkpoint.

On resume:

1. The stored resume metadata is loaded.
2. The sender computes missing chunks from the loaded checkpoint.
3. Only missing chunks are sent.
4. The receiver writes remaining chunks into the same destination file.
5. Final verification confirms the completed file.

This is restart-safe for transfer state. Persistent chunk storage beyond the destination file is not implemented in this milestone.

---

## Failure Handling

The pipeline rejects:

* invalid transfer requests
* unexpected message types
* invalid session transitions
* missing chunk metadata
* chunk hash mismatches
* file hash mismatches
* cipher failures
* file I/O failures

The MVP does not implement automatic reconnect, retry policy, crash recovery orchestration, QUIC error mapping, relay fallback, or NAT traversal. Those remain future roadmap items.
