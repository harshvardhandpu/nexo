# Nexo QUIC Transport MVP

## Purpose

The QUIC transport MVP is the first real networking implementation behind the existing transport contracts.

It provides localhost QUIC communication for:

* opening and accepting connections
* opening and accepting bidirectional streams
* sending and receiving `MessageEnvelope` values
* emitting transport events
* preserving the existing `TransportProvider`, `TransportListener`, `TransportConnection`, and `TransportStream` boundaries

It does not implement peer discovery, NAT traversal, relay nodes, mesh networking, multipath transfers, automatic reconnect, production certificate management, UI, or transfer policy.

---

## Architecture

```text
QuicTransportProvider
        |
        v
QuicListener
        |
        v
QuicConnection
        |
        v
QuicStream
        |
        v
quinn Endpoint / Connection / bidirectional stream
```

### QuicTransportProvider

Owns:

* local `PeerId`
* local bind address
* a Tokio runtime used to adapt Quinn's async API to the existing synchronous transport traits
* manually registered peer address and certificate entries

The provider does not discover peers. Callers must register a peer's socket address and trusted certificate before calling `connect`.

### QuicListener

Owns a server-side Quinn endpoint bound to a localhost socket.

It exposes:

* the bound socket address
* the generated certificate DER bytes that a peer must trust for this MVP

### QuicConnection

Wraps one Quinn connection and emits transport events through the existing `TransportEvent` model.

The client sends an internal one-way connection handshake containing its `PeerId` and `SessionId`. The listener consumes this handshake before returning `QuicConnection`, so the connection can still expose the remote peer through the transport trait.

### QuicStream

Wraps one Quinn bidirectional stream pair.

Quinn only exposes an accepted bidirectional stream after the opener sends data. To preserve the existing `open_stream` then `accept_stream` contract, `open_stream` writes a small internal stream preface and `accept_stream` consumes it before returning. Application messages start after that preface.

---

## Message Framing

The MVP frames each `MessageEnvelope` as:

```text
u32 big-endian payload length
bincode-encoded MessageEnvelope payload
```

Frames are limited to 64 MiB.

This is an internal MVP transport codec, not the stable Nexo protocol format. The technology stack still points toward Protocol Buffers for the long-term serialization layer.

---

## Connection Lifecycle

```text
Provider registers peer address + trusted certificate
  -> connect(peer, session_id)
  -> TransportEvent::Connecting
  -> QUIC handshake
  -> internal peer/session handshake
  -> TransportEvent::Connected
```

The listener flow is:

```text
listen()
  -> accept()
  -> QUIC handshake completes
  -> internal peer/session handshake is received
  -> TransportEvent::Connected
```

Closing a connection calls Quinn's connection close and emits `TransportEvent::Closed`.

Connection failure is mapped to `TransportError::ConnectionFailed`, `TransportError::ConnectionClosed`, or `TransportError::Protocol` depending on where the failure occurs.

---

## Stream Lifecycle

```text
open_stream()
  -> Quinn open_bi()
  -> write internal stream preface
  -> TransportEvent::StreamOpened
  -> send_message()
  -> TransportEvent::MessageSent
```

```text
accept_stream()
  -> Quinn accept_bi()
  -> read internal stream preface
  -> TransportEvent::StreamOpened
  -> receive_message()
  -> TransportEvent::MessageReceived
```

Closing a stream finishes the send side and emits `TransportEvent::StreamClosed`.

Multiple streams may be opened on the same connection. Ordering is guaranteed only within a single stream; different streams are independent.

---

## Error Handling

The QUIC transport maps errors into the existing non-policy-bearing transport errors:

* connection setup failures become `ConnectionFailed`
* operations on closed connections become `ConnectionClosed`
* stream open, read, write, finish, and preface failures become `StreamFailed`
* invalid message frames or codec failures become `MessageRejected`
* malformed internal transport prefaces become `Protocol`

The transport layer does not decide whether a transfer should pause, retry, fail, or cancel. Session and engine layers retain that policy.

---

## Security Assumptions

QUIC uses TLS 1.3 through Quinn and Rustls.

For this MVP:

* listeners generate self-signed localhost certificates
* clients trust a peer only when the caller explicitly registers that peer's certificate
* certificate trust is sufficient for localhost integration tests and early local development
* long-term identity, pairing, certificate pinning policy, and user-visible trust decisions are future work

The QUIC transport secures the transport channel. End-to-end transfer payload encryption remains owned by the crypto and engine boundaries described in `docs/crypto-layer.md` and `docs/transfer-pipeline.md`.

---

## Future NAT Traversal Compatibility

This MVP intentionally uses explicit socket addresses and localhost binding.

Future discovery or NAT traversal layers can populate the same peer registration boundary with externally learned addresses and trusted peer credentials. The QUIC transport should remain focused on:

* binding endpoints
* connecting to known peer addresses
* accepting incoming connections
* multiplexing streams
* moving framed transfer messages
* reporting transport events

It should not absorb relay routing, hole punching, peer discovery, mesh routing, or reconnect policy.
