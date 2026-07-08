# Nexo 2.0 — Global Peer-to-Peer Share Links Architecture

> Status: **design / forward-looking**. This document describes a target
> architecture for Nexo 2.0. It implements nothing and changes no current code.
> It is a north star for the transition from local device sharing to global,
> link-based sharing — **without** becoming cloud storage.

---

## Vision

Transform Nexo from:

> **"AirDrop for your devices"** — direct transfer between devices you own on the
> same local network.

into:

> **"A decentralized WeTransfer alternative without cloud storage."** — send a
> file to *anyone, anywhere* by handing them a link, while the bytes still travel
> **directly** peer-to-peer and are **never** stored on a Nexo server.

The user experience target: *"Copy a link. Send it however you like. The
recipient opens it and the file streams straight from your device to theirs,
encrypted end to end."*

## Core principle (non-negotiable)

**Nexo must never become a cloud storage service.**

1. **Files are never uploaded to Nexo infrastructure.** No server ever holds
   file bytes, plaintext or ciphertext, at rest.
2. **Transfers remain direct, device-to-device.** The QUIC data path in Nexo 2.0
   is the same one shipped in 1.0.
3. **The server layer only provides temporary coordination** — discovery,
   rendezvous, signaling, and (optionally) relaying of *encrypted* bytes it
   cannot read and does not persist. Coordination state is ephemeral and expires.

Everything below is subordinate to this principle. Any design choice that would
require durable server-side storage of file contents is explicitly rejected.

---

## Nexo 1.0 Architecture (Current)

Nexo 1.0 is a Rust workspace with a Tauri + React desktop layer. The transfer
engine is frozen; the desktop app only calls its public APIs.

### Layers

| Layer | Crate | Responsibility |
|---|---|---|
| Transport | `crates/networking` | QUIC (via `quinn`) + mDNS discovery (`_nexo._udp.local.`) |
| Engine | `crates/engine` | chunking (4 MiB default), transfer pipeline, SHA-256 verification |
| Storage | `crates/storage` | SQLite checkpoint / resume / session persistence |
| Crypto | `crates/crypto` | X25519 ephemeral key exchange → ChaCha20-Poly1305 AEAD |
| Common | `crates/common` | shared types + transfer protocol messages |
| Orchestration | `crates/cli` | CLI + transfer orchestration |
| Desktop | `apps/desktop` | Tauri (Rust bridge) + React UI |

### How a 1.0 transfer works

```
   Sender device                         Receiver device
 ┌──────────────┐   mDNS: _nexo._udp   ┌──────────────┐
 │ discover ────┼─────────────────────▶│ advertise    │
 │              │                      │ (ServiceAdv.) │
 │ send FILE ──▶│  1. sender approves  │              │
 │              │  2. receiver approves │◀── accept    │
 │  QUIC (TLS 1.3, keep-alive) ═════════│              │
 │  request → chunk metadata → X25519   │              │
 │  key exchange → encrypted chunks ───▶│  write +     │
 │  → FileVerified → Acknowledged       │  SHA-256     │
 └──────────────┘                      └──────────────┘
    resume via SQLite checkpoints if interrupted
```

Key properties Nexo 2.0 must preserve:

- **Direct QUIC data path.** `quinn` endpoints, self-signed per-listener
  certificates trusted out-of-band, keep-alive to survive long verification
  stalls.
- **Explicit consent.** Both sender and receiver approve every transfer;
  trusted-device auto-accept is opt-in and never bypasses certificate trust.
- **Integrity + resume.** Per-chunk and whole-file SHA-256; incremental
  checkpoints let an interrupted transfer resume mid-file.
- **Local-only trust today.** Certificates are exchanged by advertising them on
  the LAN; the sender pins the receiver's certificate. There is **no** global
  identity or discovery.

### What 1.0 cannot do (and why 2.0 exists)

- **No transfer beyond the LAN.** mDNS is link-local; peers must be on the same
  network segment.
- **No addressing of a peer you can't already see.** Trust is bootstrapped by
  physically-present certificate advertisement.
- **No NAT traversal.** Both peers are assumed directly reachable.
- **No asynchronous handoff.** Both peers must be online simultaneously.

Nexo 2.0 closes the *reachability* and *addressing* gaps **without** closing the
*"no cloud storage"* principle.

---

## Nexo 2.0 Architecture (Target)

### The one new idea: a Share Link is a capability, not a file

A **share link** encodes everything a recipient needs to *find* and *decrypt* a
transfer — but never the transfer itself:

```
https://nexo.link/#<transfer-id>.<link-secret>
                    └─ rendezvous key   └─ end-to-end key material
```

- Everything after `#` is a **URL fragment**: browsers never send it to the
  server, so the coordination server cannot learn the `link-secret`.
- `transfer-id` names an ephemeral **rendezvous slot** on the coordination
  server.
- `link-secret` is the seed for the **end-to-end key** and an authenticator; the
  server never sees it.

Whoever holds the link can (a) find the sender via rendezvous and (b) derive the
key to decrypt the direct stream. The link *is* the capability. The sender can
revoke it by closing the rendezvous slot.

### New component: the Coordination Server (stateless-ish, storage-free)

A small, horizontally-scalable service that provides **only** ephemeral
coordination. Think "signaling + rendezvous + optional dumb relay," not storage.

Responsibilities (all time-boxed, all in-memory / short-TTL):

1. **Rendezvous** — map a `transfer-id` to the sender's current reachability
   candidates (IP/port sets, relay tokens). TTL measured in minutes.
2. **Signaling** — pass small, opaque, end-to-end-encrypted control blobs
   between the two peers to bootstrap a direct QUIC connection (ICE-style
   candidate exchange).
3. **Optional relay (TURN-like)** — when direct connectivity is impossible,
   forward **encrypted** QUIC/UDP datagrams it cannot decrypt and does not
   persist. Relayed bytes are billed against a rate/size budget and dropped after
   delivery.

Explicit non-responsibilities (rejected by the core principle):

- ❌ Storing files (plaintext or ciphertext) at rest.
- ❌ Holding a transfer for a later, offline recipient beyond a short rendezvous
  TTL.
- ❌ Seeing filenames, sizes-as-content, or the `link-secret`.
- ❌ Being required for LAN transfers — 1.0 mDNS direct mode still works with no
  server at all.

```
        Sender                Coordination Server               Recipient
          │  register(transfer-id, candidates, exp)  │              │
          │─────────────────────────────────────────▶│              │
          │                                          │  open link   │
          │                                          │◀─────────────│
          │       signaling: opaque E2E blobs (ICE)  │              │
          │◀────────────────────────────────────────▶│◀────────────▶│
          │                                                         │
          │══════════ direct QUIC (hole-punched) ═══════════════════│
          │   OR ═══ relayed encrypted datagrams (server = blind) ══│
          │        same 1.0 engine: chunks + SHA-256 + resume       │
```

### Identity & addressing

1.0 has no global identity. 2.0 introduces a **portable device keypair**
(long-lived Ed25519), distinct from the ephemeral per-session X25519 keys:

- The device public key is the stable, global **address** of a peer.
- `transfer-id` is derived from a fresh per-transfer key, so links are
  unlinkable to the device identity unless the sender chooses otherwise.
- Trust upgrades from "certificate seen on the LAN" to "**pinned device public
  key**, optionally cross-signed by prior transfers" — a TOFU (trust-on-first-use)
  model with an explicit trusted-devices list (the 1.0 trust UI generalizes to
  this).

The certificate-pinning discipline of 1.0 is preserved: the direct QUIC session
still authenticates via certificates, now bound to the device keypair rather than
a LAN-advertised self-signed cert.

### End-to-end encryption model

The data path keeps 1.0's AEAD (ChaCha20-Poly1305) but rekeys around the link:

```
link-secret ─(HKDF)─▶ handshake auth key + transfer root key
                                 │
        X25519 ephemeral DH ─────┴──▶ per-session key ──▶ ChaCha20-Poly1305 chunks
```

- The `link-secret` authenticates the recipient to the sender (they hold the
  capability) and salts key derivation, so a relay or the coordination server —
  which never sees the fragment — cannot derive session keys.
- Forward secrecy comes from the ephemeral X25519 exchange, exactly as in 1.0.
- The relay, if used, sees only ciphertext QUIC datagrams.

### NAT traversal (the hard networking problem)

Global reach requires getting two peers behind NATs to talk directly:

1. **Candidate gathering** — each peer collects host, server-reflexive (STUN-like,
   via the coordination server), and relay candidates.
2. **Hole punching** — exchange candidates over signaling, attempt simultaneous
   open. QUIC's connection migration helps here.
3. **Relay fallback** — if punching fails (symmetric NAT on both ends), fall back
   to the TURN-like encrypted relay. The relay is **the exception, not the
   path**, and is explicitly budgeted so it can never become "cloud transfer as a
   service."

### The asynchronous-transfer problem (and why we don't solve it with storage)

WeTransfer's core trick is *time-shifting*: upload now, download later. That
**requires** durable storage — which we reject. Nexo 2.0's honest answer:

- **Both peers online → direct or relayed live transfer.** This is the primary
  mode.
- **Recipient offline → the transfer waits on the sender, not the server.** The
  rendezvous slot holds *reachability*, not *bytes*; if it expires before the
  recipient connects, the link simply needs the sender online again.
- **Optional user-owned "always-on" node.** A user who wants time-shifting runs
  their *own* Nexo node (desktop kept awake, a home server, later a self-hosted
  headless daemon) that holds the file. This keeps storage on **user-owned**
  hardware — never Nexo's. This is a feature, not a hosted service.

This is the central trade-off of the vision and is called out deliberately: we
trade "upload-and-forget" convenience for "your files never touch our servers."

---

## Security & privacy considerations

- **Link secrecy = fragment secrecy.** The whole model rests on the `#fragment`
  never reaching the server. UI must make "this link grants access" obvious and
  support one-time / expiring / revocable links.
- **Capability, not ACL.** Anyone with the link can fetch until the sender
  revokes or it expires. Mitigations: short TTLs, one-time links, optional
  recipient device-key pinning ("only this device may claim the link").
- **Metadata minimization.** The coordination server should learn as little as
  possible: opaque `transfer-id`, coarse timing, candidate IPs (unavoidable for
  rendezvous). No filenames, no sizes-as-content, no link secret.
- **Relay is blind and budgeted.** Encrypted datagrams only; strict per-transfer
  size/time budget; nothing persisted; abuse-rate-limited.
- **Threat model additions vs 1.0:** malicious coordination server (must not be
  able to read files or impersonate peers), link interception (expiry/one-time
  links), relay abuse (budgets), Sybil/spam on rendezvous (proof-of-work or
  authenticated slots).
- **Preserved from 1.0:** explicit consent both directions, certificate/device-key
  pinning never bypassed, per-chunk + whole-file SHA-256, no silent corruption.

---

## What stays exactly the same

Nexo 2.0 is an **additive** layer. The following 1.0 components are reused
unchanged:

- The QUIC data path (`crates/networking` transport).
- The chunking / pipeline / SHA-256 engine (`crates/engine`).
- Checkpoint + resume (`crates/storage`).
- AEAD chunk encryption (`crates/crypto`).
- Sender/receiver approval, trust list, history, tray, background receiver.
- **LAN direct mode** works with no coordination server at all — 2.0 degrades
  gracefully to 1.0 behavior on a local network.

The coordination server, share links, device identity, NAT traversal, and relay
are **new and optional** additions around that stable core.

---

## Delivery phases (suggested, non-binding)

Each phase is independently shippable and preserves the core principle.

1. **Phase A — Device identity & pinning.** Long-lived device keypair; generalize
   the trust list to pinned device keys. No server yet. (Enables everything
   later; testable on the LAN.)
2. **Phase B — Coordination server (rendezvous + signaling only).** Minimal
   service; direct WAN transfer when NAT allows; no relay. Share links (v1):
   both peers online, direct path.
3. **Phase C — NAT traversal.** STUN-like reflexive candidates + hole punching;
   measure success rate across NAT types.
4. **Phase D — Blind relay fallback.** TURN-like encrypted relay with strict
   budgets, for the cases Phase C can't punch through.
5. **Phase E — User-owned always-on node.** Optional headless daemon so a user
   can self-host time-shifted transfers on their own hardware.

Non-goals for 2.0 (explicitly out of scope): hosted storage, accounts as a
requirement, server-side file scanning, any design that persists file bytes on
Nexo infrastructure.

---

## Open questions (to resolve during design)

- Rendezvous slot TTL and revocation semantics — how long is "temporary"?
- Coordination-server trust: single operator vs. federated vs. self-hostable.
  Federation best matches the "not a cloud service" ethos.
- Abuse resistance on rendezvous/relay without introducing mandatory accounts.
- Link format versioning and forward compatibility.
- How the desktop UI communicates the capability model ("a link is a key") to
  non-technical users without footguns.

---

## Summary

Nexo 2.0 extends Nexo from local to global by adding a **storage-free
coordination layer** and **capability-based share links**, while keeping the
direct, encrypted, resumable QUIC transfer engine of 1.0 exactly as-is. The
coordination server brokers *meetings*, optionally forwards *ciphertext it cannot
read*, and stores *nothing*. The result is a decentralized WeTransfer
alternative that is, by construction, incapable of becoming cloud storage.
