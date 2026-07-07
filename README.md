<div align="center">

# Nexo

### Fast, encrypted, peer-to-peer file transfer

Move large files directly between your devices over the local network —
**end-to-end encrypted**, **resumable**, and **cloud-free**. Like AirDrop, but
cross-platform and open.

</div>

---

## Why Nexo

Most "quick share" tools bounce your files through a cloud server, cap your file
size, or fall over on a flaky connection. Nexo connects your devices **directly**
over an encrypted [QUIC](https://www.chromium.org/quic/) channel, verifies every
byte with SHA-256, and **resumes exactly where it left off** if the link drops —
so a 5 GB transfer survives a closed laptop lid.

## Features

- ✅ **QUIC transport** — modern, multiplexed, encrypted-by-default UDP transport
- ✅ **Resume** — crash-safe, incremental checkpoints; interrupted transfers pick up mid-file
- ✅ **End-to-end encryption** — per-session keys; the transport is TLS 1.3 (QUIC)
- ✅ **No cloud** — files go device→device; nothing is uploaded to a server
- ✅ **LAN discovery** — devices find each other automatically over mDNS
- ✅ **Explicit consent** — both sender and receiver approve every transfer; trusted-device auto-accept is opt-in
- ✅ **Integrity verified** — per-chunk + whole-file SHA-256; zero silent corruption
- ✅ **Desktop app** — premium dark UI, system tray, background receiver, notifications, onboarding
- ✅ **Cross-platform** — Linux (AppImage/deb) and Windows (MSI)

## How it works

```
   Device A                          Device B
 ┌──────────┐   mDNS discovery     ┌──────────┐
 │  Nexo    │◀────────────────────▶│  Nexo    │
 │          │                      │          │
 │  send ──▶│  1. sender approves  │          │
 │          │  2. receiver approves│◀─ accept │
 │          │═════ QUIC (TLS 1.3) ═│          │
 │          │  chunked + SHA-256   │          │
 │          │  resumable transfer  │─▶ file   │
 └──────────┘                      └──────────┘
        no cloud · no account · encrypted end to end
```

1. **Discover** — devices advertise over mDNS on the local network.
2. **Request** — the sender picks a device and confirms.
3. **Approve** — the receiver accepts (or auto-accepts a trusted device).
4. **Transfer** — the file is chunked, encrypted, streamed over QUIC, and
   verified. If interrupted, it resumes from the last checkpoint.

## Screenshots

> _Desktop app — Midnight Flow theme (dark, glass, cyan→purple)._
>
> Dashboard · Devices · Send (drag & drop) · Transfer monitor · Trusted devices ·
> History · Settings · Onboarding.
>
> _(Add PNGs under `docs/screenshots/` and link them here when capturing on a
> machine with a display.)_

## Install

**Linux** — download the AppImage or `.deb` from the
[latest release](https://github.com/harshvardhandpu/nexo/releases):

```bash
# AppImage (any distro)
chmod +x Nexo-linux.AppImage && ./Nexo-linux.AppImage

# Debian / Ubuntu
sudo apt install ./Nexo-linux.deb
```

Full instructions (dependencies, tray setup, autostart, troubleshooting):
[`docs/linux-install.md`](docs/linux-install.md).

**Windows** — run the `Nexo-windows.msi` installer.

## Command line

Nexo also ships a CLI (used by the desktop app under the hood):

```bash
nexo --version
nexo receive               # advertise + receive (asks to accept each transfer)
nexo discover              # list nearby devices
nexo send FILE --host ADDR # send to a discovered device
```

## Build from source

Prerequisites: Rust (stable), Node ≥ 18, and the Linux system libraries in
[`docs/linux-install.md`](docs/linux-install.md).

```bash
git clone https://github.com/harshvardhandpu/nexo
cd nexo

# Rust workspace (core engine + CLI)
cargo test --workspace
cargo run -p cli -- --help

# Desktop app
cd apps/desktop
npm ci
npm run tauri dev          # develop
npm run tauri build        # package AppImage + deb
```

## Architecture

Nexo is a Rust workspace with a thin Tauri + React desktop layer on top:

| Crate / dir | Responsibility |
|---|---|
| `crates/networking` | QUIC transport + mDNS discovery |
| `crates/engine` | chunking, transfer pipeline, SHA-256 verification |
| `crates/storage` | SQLite checkpoint / resume / session persistence |
| `crates/crypto` | session key exchange + AEAD |
| `crates/common` | shared types + transfer protocol messages |
| `crates/cli` | orchestration + command-line app |
| `apps/desktop` | Tauri (Rust bridge) + React UI |

The transfer engine is stable and frozen; the desktop app only calls its public
APIs. See [`docs/`](docs/) for the protocol, transport, session, update system,
and release process.

## Security

- Transport is QUIC (TLS 1.3); payloads are additionally encrypted with
  per-session keys.
- Every transfer requires explicit sender **and** receiver approval. Auto-accept
  is limited to devices you have explicitly trusted and is off by default.
- Certificate trust is never bypassed by any UI setting.

## Status

**1.0 Release Candidate.** Core transfer engine complete; desktop app feature
complete (tray, background receiver, notifications, onboarding, autostart,
diagnostics). See [`docs/release-checklist.md`](docs/release-checklist.md).

## License

See repository.
