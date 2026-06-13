# Nexo Loopback Transport

## Purpose

The loopback transport is an in-memory implementation of the existing transport contracts.

It exists so Nexo can test session and transfer message flow without sockets, QUIC, NAT traversal, relay nodes, or any external network dependency.

The loopback transport is part of the networking crate because it is a concrete transport implementation. It is not a storage layer, crypto layer, transfer engine, or UI feature.

---

## Architecture

```text
LoopbackTransportProvider
        |
        v
LoopbackNetwork registry
        |
        v
LoopbackListener
        |
        v
LoopbackConnection
        |
        v
LoopbackStream
```

### LoopbackTransportProvider

Owns a local `PeerId` and a shared in-memory loopback network registry.

Responsibilities:

* register a listener for the local peer
* connect to a listening peer
* create paired in-memory connections

### LoopbackListener

Accepts incoming loopback connections through an in-memory queue.

### LoopbackConnection

Represents one side of a paired in-memory connection.

Responsibilities:

* expose local connection metadata through the transport contract
* open streams
* accept streams opened by the peer
* emit transport events
* close the connection

### LoopbackStream

Represents one side of a paired in-memory stream.

Responsibilities:

* send `MessageEnvelope` values to the paired stream
* receive `MessageEnvelope` values from the paired stream
* emit message and stream events

---

## Message Flow

```text
Sender provider
    |
    | connect(receiver peer)
    v
Receiver listener
    |
    | accept()
    v
Paired connections
    |
    | open_stream()
    v
Paired streams
    |
    | send_message(MessageEnvelope)
    v
receive_message()
```

Connection creation emits transport events such as:

* `Connecting`
* `Connected`

Stream and message operations emit:

* `StreamOpened`
* `MessageSent`
* `MessageReceived`
* `StreamClosed`
* `Closed`

Message delivery is in-memory and process-local. Messages are cloned into channel queues and delivered only while both paired endpoints remain alive.

---

## Testing Strategy

The loopback transport is intended for deterministic tests that need transport behavior without real networking.

Tests should prove:

* a sender can connect to a receiver
* streams can be opened and accepted
* messages are delivered across streams
* bidirectional stream communication works
* transport events are generated
* multiple simultaneous streams remain independent
* session state can be driven by transport events
* storage can persist session/checkpoint state observed during loopback message flow

The current tests use the storage crate only as a test dependency of the networking crate. Production networking code does not depend on storage.

---

## Limitations

The loopback transport does not implement:

* sockets
* QUIC
* mDNS
* peer discovery over a real network
* NAT traversal
* relay behavior
* authentication
* encryption
* retransmission
* ordering beyond in-memory channel ordering per stream
* crash recovery

It is not a production transport. It is a foundation tool for end-to-end testing of Nexo's transport/session/storage contracts before real networking is introduced.
