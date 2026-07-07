# Nexo Update System (Design)

> Status: **design / foundation**. This documents the intended update mechanism.
> It does not modify the transfer engine, protocol, storage, or crypto layers.

Nexo is a local-first desktop application distributed as an AppImage, `.deb`,
and (later) a Windows installer. This document describes how future versions
will be checked for, delivered, and migrated to, safely.

## Goals

- Users learn about new versions without manual checking.
- Updates are **authenticated** — only releases signed by the Nexo maintainers
  are ever applied.
- Updates never risk in-flight or resumable transfers, trusted-device state, or
  history.
- Works across AppImage / `.deb` / MSI, respecting each platform's norms.

## 1. Version checking

- The app knows its own version from `tauri.conf.json` (`version`) exposed to
  the UI via a `get_app_version` command.
- A lightweight, opt-in check queries a static, versioned manifest hosted over
  HTTPS (e.g. `https://releases.nexo.dev/latest.json`):

  ```json
  {
    "version": "0.2.0",
    "pub_date": "2026-08-01T00:00:00Z",
    "notes": "…",
    "platforms": {
      "linux-x86_64": {
        "url": "https://releases.nexo.dev/Nexo_0.2.0_amd64.AppImage",
        "signature": "<minisign/ed25519 signature>"
      },
      "windows-x86_64": { "url": "…", "signature": "…" }
    }
  }
  ```

- Checks are **rate-limited** (at most once per launch + once/day) and can be
  disabled in Settings → General. No telemetry is sent; the request carries only
  the current version and platform.

## 2. Signed releases

- Every artifact is signed with an offline **ed25519** key (via Tauri's updater
  signing or `minisign`). The public key is compiled into the app.
- The updater **verifies the signature before applying** anything. A failed or
  missing signature aborts the update with a clear error — no partial writes.
- The signing key never lives in CI in plaintext; releases are signed on a
  maintainer machine and only the signature + artifact are published.

## 3. Delivery per platform

| Platform | Mechanism |
|---|---|
| AppImage (Linux) | Download new AppImage, verify signature, swap in place, relaunch. Optionally integrate with `AppImageUpdate` deltas later. |
| `.deb` (Linux) | Prefer the system package manager; the app surfaces "a new version is available" and links to the repo/download rather than self-replacing a managed file. |
| MSI/NSIS (Windows) | Tauri updater downloads + runs the signed installer, then relaunches. |

The app **detects its install kind** (AppImage via `$APPIMAGE`, deb via package
metadata, etc.) and only self-updates where that is safe; otherwise it notifies.

## 4. Migration handling

Application-level state lives as versioned JSON under the app data dir
(`trusted-devices.json`, `transfer-history.json`, `background-settings.json`,
`preferences.json`, `onboarding.json`). Migration rules:

- Each file gains a `schemaVersion` field; readers upgrade older shapes forward
  and **never** hard-fail on unknown/missing fields (today they already degrade
  to defaults on parse error).
- The **core transfer state** (SQLite database, `receiver.identity`,
  `receiver.peer`, checkpoints, resume metadata) is owned by the unchanged
  engine/storage layers and is migrated only through their existing schema
  mechanisms — the update layer must not touch it directly.
- Before applying an update, the app records the outgoing version so a downgrade
  path can detect and warn about forward-incompatible state.

## 5. In-flight safety

- Updates are only *applied* (swap + relaunch) when **no transfer is running**
  (`get_receiver_status.receiving == false` and no active send job).
- A resumable transfer interrupted by an update resumes via the existing
  checkpoint/resume system after relaunch — the update layer relies on, and does
  not alter, that guarantee.

## Out of scope for this phase

- No auto-apply. This phase ships version *awareness* only; applying updates is
  a later milestone once release infrastructure + signing keys exist.
