# Nexo Peer Discovery MVP

## Purpose

Peer discovery lets Nexo peers see other Nexo instances on the same local network without central infrastructure.

This MVP is intentionally limited to LAN visibility. It does not establish trust, connect transfers automatically, traverse NAT, use relays, or perform internet discovery.

---

## Architecture

Discovery lives in the networking crate.

```text
PeerDiscovery
      |
      v
LocalDiscoveryProvider
      |
      v
mDNS service advertisement and browse
```

### PeerDiscovery

The trait exposes:

* `next_event(timeout)` for discovery events
* `peers()` for the current peer cache
* `shutdown()` for explicit service cleanup

### LocalDiscoveryProvider

The local provider:

* advertises a local `PeerAdvertisement`
* browses for `_nexo._udp.local.`
* ignores its own peer ID
* validates the advertised discovery version
* normalizes resolved addresses
* stores visible peers in an in-memory cache
* expires peers when mDNS removal events or cache timeout indicate the peer is gone

---

## Data Model

### PeerAdvertisement

Published by a local peer:

* peer ID
* display name
* port

### PeerInfo

Stored for discovered peers:

* peer ID
* display name
* resolved IP addresses
* advertised port

### DiscoveryEvent

Events emitted by a provider:

* `PeerDiscovered`
* `PeerUpdated`
* `PeerExpired`

---

## CLI Integration

`nexo discover` creates or loads a persistent local peer ID from the CLI state directory, advertises it over mDNS during a short scan window, and prints discovered peer display names.

The persistent peer ID is stored in:

```text
peer-id
```

It is an opaque discovery identifier only. It is not a cryptographic identity, trust root, pairing credential, or QUIC certificate.

---

## Boundaries

Peer discovery does not:

* modify transfer engine behavior
* own cryptographic primitives
* persist checkpoints or sessions
* establish QUIC certificate trust
* choose relay routes
* implement NAT traversal
* perform DHT or internet discovery
* start transfers automatically

Future pairing or trust layers may use discovered peer metadata as an input, but authentication and transfer policy must remain outside the discovery provider.

---

## Testing

The default test suite covers:

* listing cached peers
* ignoring the local peer
* handling peer removal
* handling peer updates
* expiring stale peers
* CLI peer ID persistence
* CLI discovery output

An ignored mDNS smoke test is available for manual local verification because multicast loopback behavior varies by operating system, network interface, and host configuration.
