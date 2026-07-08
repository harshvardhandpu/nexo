# Nexo v1.0.0

**Fast, private, peer-to-peer file transfers without cloud storage.**

Nexo transfers files **directly** between your devices over an encrypted
connection. Nothing is uploaded to a cloud server — because there isn't one
holding your files. This is the first stable release.

---

## Highlights

- 🚀 **Direct device-to-device transfers** over encrypted QUIC — no cloud, no
  accounts, no size cap imposed by someone else's disk.
- 🔒 **End-to-end encrypted** connections with per-transfer keys.
- ⏯️ **Resume that actually works** — a dropped connection or closed laptop lid
  continues the transfer instead of restarting it.
- 🖥️ **Polished cross-platform desktop app** — Linux and Windows, with a system
  tray, background receiving, notifications, and a guided first-run.
- ✅ **Consent on both sides** — every transfer is approved by the sender *and*
  the receiver.

## Features

- **Direct peer-to-peer transfers** — files stream device to device.
- **End-to-end encrypted** — QUIC / TLS 1.3 transport plus per-session AEAD.
- **Resume interrupted transfers** — crash-safe incremental checkpoints.
- **Large file support** — multi-gigabyte transfers (validated at 1 GB and 5 GB).
- **LAN discovery** — nearby devices find each other automatically over mDNS.
- **Device trust** — remember devices, view certificate fingerprints, rename or
  revoke trust.
- **Transfer approval** — sender and receiver both confirm; optional auto-accept
  for trusted devices (off by default).
- **Background receiving** — stay available from the system tray with the window
  closed.
- **Desktop niceties** — onboarding, notifications, autostart, custom download
  folder, transfer history, and a diagnostics panel.

## Security

- Files are transferred **directly** between devices and are **never** uploaded
  to a Nexo server.
- The transport is **QUIC (TLS 1.3)**; file chunks are additionally encrypted
  with **per-session keys** (X25519 key exchange → ChaCha20-Poly1305 AEAD).
- Integrity is verified with **per-chunk and whole-file SHA-256** — a corrupted
  transfer is rejected, never silently written.
- **Every transfer requires explicit approval** from both sides. Auto-accept is
  limited to devices you have explicitly trusted and is disabled by default;
  certificate trust is never bypassed by a UI setting.

> Note: these builds are **not yet code-signed**, so Windows SmartScreen /
> antivirus may warn on first run. See the install guides for how to proceed and
> why. Code signing is planned for a future release.

## Supported platforms

| Platform | Status | Artifact |
|---|---|---|
| Linux (x86_64) | ✅ Supported | `Nexo-linux.AppImage`, `Nexo-linux.deb` |
| Windows 10/11 (x64) | ✅ Supported | `Nexo-windows.msi` |
| macOS | ⏳ Coming later | — |

## Installation

**Linux**

```bash
# AppImage (any distro)
chmod +x Nexo-linux.AppImage && ./Nexo-linux.AppImage

# Debian / Ubuntu
sudo apt install ./Nexo-linux.deb
```

Full guide: [`docs/linux-install.md`](linux-install.md).

**Windows** — download `Nexo-windows.msi`, run the installer, launch **Nexo**
from the Start Menu. Full guide: [`docs/windows-install.md`](windows-install.md).

**macOS** — coming later.

## Known limitations

- **LAN transfers only.** v1.0 transfers between devices on the **same local
  network**. Discovery uses mDNS; direct transfer needs the two devices to reach
  each other on the LAN. Internet **share links** (send to anyone, anywhere, with
  a link — still peer-to-peer, still no cloud storage) are planned for **Nexo
  2.0**; see [`docs/roadmap/nexo-2-share-links.md`](roadmap/nexo-2-share-links.md).
- **No code signing yet.** Unsigned installers trigger SmartScreen / antivirus
  warnings on Windows; signing is planned.
- **No auto-update yet.** Update the app by downloading a newer release. The
  update-system design is documented in [`docs/update-system.md`](update-system.md).
- **macOS not yet available.**
- **VPNs / restrictive networks** can block LAN discovery; use the manual address
  field, or see the install-guide troubleshooting.

## Thanks

Nexo 1.0 is the result of a long hardening effort across the transport, resume,
storage, and desktop layers. Feedback and bug reports are very welcome — please
use the issue templates.
