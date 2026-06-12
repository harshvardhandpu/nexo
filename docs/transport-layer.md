# Nexo Transport Layer

## Purpose

The transport layer defines how future networking implementations move session and transfer messages.

It sits below the session layer and above concrete transports such as QUIC, relay transport, loopback transport, or test transport.

The transport layer is a contract only. It does not implement QUIC, sockets, peer discovery, NAT traversal, relay nodes, Tauri, React, storage, or encryption.

---

## Responsibilities

The transport layer is responsible for:

* Opening and accepting transport connections
* Creating logical streams within a connection
* Sending and receiving framed transfer messages
* Reporting connection, stream, message, and failure events
* Preserving session and transfer identifiers across message flow
* Exposing transport errors without deciding session policy

The transport layer is not responsible for:

* Peer discovery
* Session acceptance decisions
* Chunk scheduling
* File hashing
* Checkpoint persistence
* Encryption or key exchange
* Retry policy beyond what a concrete transport natively provides

---

## Connection Lifecycle

Connections are identified by `ConnectionId`.

Expected lifecycle:

```text
Connecting
  -> Connected
  -> StreamOpened
  -> MessageReceived / MessageSent
  -> StreamClosed
  -> Closed
```

Failures may be reported at any point as a transport event.

Connection closure does not imply transfer failure by itself. The session layer decides whether a transfer can pause, resume, fail, or cancel.

---

## Stream Lifecycle

Streams are logical channels within a transport connection.

The contract allows future transports to use:

* One stream for session messages
* One or more streams for chunk messages
* Separate streams for control and verification messages

Expected lifecycle:

```text
StreamOpened
  -> MessageReceived / MessageSent
  -> StreamClosed
```

The contract does not require bidirectional streams, unidirectional streams, or any QUIC-specific behavior.

---

## Message Flow

All messages are wrapped in a `MessageEnvelope`.

The envelope carries:

* Session ID
* Transfer ID
* Message category
* Message body

Message categories:

* Session messages
* Control messages
* Chunk messages
* Verification messages

Session messages create and answer transfer requests.

Control messages coordinate pause, resume, cancel, and checkpoint-related control flow.

Chunk messages carry chunk metadata or chunk payload bytes.

Verification messages carry integrity verification results.

---

## Error Handling

Transport errors should be explicit and non-policy-bearing.

Examples:

* Connection failed
* Connection closed
* Stream failed
* Message rejected
* Timeout
* Protocol error

The transport layer reports errors. The session layer decides state transitions such as `Paused`, `Failed`, or `Cancelled`.

---

## Reliability Guarantees

The transport contract guarantees only that implementations report their outcomes through transport events.

The contract does not guarantee:

* Delivery
* Ordering
* Exactly-once semantics
* Automatic reconnection
* Crash recovery

Concrete transports may provide stronger guarantees. The engine and storage layers must still rely on chunk hashes, manifests, checkpoints, and resume metadata for correctness.

---

## Future QUIC Mapping

Future QUIC implementations can map the contract as follows:

* `TransportProvider` maps to QUIC endpoint setup
* `TransportListener` maps to incoming QUIC connection acceptance
* `TransportConnection` maps to one QUIC connection
* `TransportStream` maps to one QUIC stream
* `MessageEnvelope` maps to framed bytes sent over QUIC streams
* Chunk messages may use multiple streams for parallel transfer

This mapping is intentionally deferred. The current contract must remain independent of QUIC-specific APIs.
