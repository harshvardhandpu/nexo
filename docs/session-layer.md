# Nexo Session Layer

## Purpose

The session layer defines the transfer lifecycle between networking and the transfer engine.

```text
networking
    |
session
    |
engine
```

It does not implement QUIC, peer discovery, relay behavior, NAT traversal, storage, Tauri, or React.

The layer provides crate-neutral types and state transitions that future networking and UI code can use to coordinate a transfer safely.

---

## Core Types

### PeerId

Opaque identifier for a peer.

Properties:

* Stable for the lifetime of a connection or discovered peer entry
* Not tied to a transport address
* Does not imply authentication by itself

### SessionId

Opaque identifier for one transfer session.

Properties:

* Unique per transfer session
* Created before connection or acceptance
* Used to correlate transfer requests, responses, state, and persistence

### TransferRequest

Request sent by the sender to start a transfer.

Fields:

* Session ID
* Transfer ID
* Sender Peer ID
* Receiver Peer ID
* File manifest

The receiver validates this request before accepting.

### TransferAcceptance

Receiver response indicating the transfer can begin.

Fields:

* Session ID
* Accepted transfer ID

### TransferRejection

Receiver response indicating the transfer will not begin.

Fields:

* Session ID
* Rejection reason

### TransferResponse

Union of accepted or rejected transfer responses.

### SessionInfo

Current session metadata.

Fields:

* Session ID
* Transfer ID
* Local Peer ID
* Remote Peer ID
* Session state

---

## SessionState

Supported states:

* Created
* Connecting
* PendingAcceptance
* Accepted
* Transferring
* Paused
* Verifying
* Completed
* Failed
* Cancelled

Terminal states:

* Completed
* Failed
* Cancelled

Terminal states cannot transition to another state.

---

## State Transitions

Valid transitions:

```text
Created
  -> Connecting
  -> Cancelled
  -> Failed

Connecting
  -> PendingAcceptance
  -> Cancelled
  -> Failed

PendingAcceptance
  -> Accepted
  -> Cancelled
  -> Failed

Accepted
  -> Transferring
  -> Cancelled
  -> Failed

Transferring
  -> Paused
  -> Verifying
  -> Cancelled
  -> Failed

Paused
  -> Transferring
  -> Cancelled
  -> Failed

Verifying
  -> Completed
  -> Failed

Completed
  -> terminal

Failed
  -> terminal

Cancelled
  -> terminal
```

Invalid transitions must be rejected explicitly.

Examples:

* Created -> Transferring is invalid
* PendingAcceptance -> Transferring is invalid
* Completed -> Transferring is invalid
* Failed -> Connecting is invalid

---

## Architecture Boundary

The session layer owns transfer lifecycle state only.

It does not own:

* QUIC connections
* Peer discovery
* Relay routing
* NAT traversal
* Chunk reading or hashing
* Checkpoint persistence
* Encryption or key exchange

Networking will use session identifiers and transfer requests to move bytes between peers.

The transfer engine will use accepted sessions to schedule chunks, report progress, verify integrity, and produce checkpoints.
