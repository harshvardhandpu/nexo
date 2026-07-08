# Nexo Release Checklist

The final sign-off checklist for cutting a Nexo desktop release. The core engine
(networking/QUIC, storage, engine, crypto, resume, protocol) is frozen — only the
desktop app, packaging, and docs change.

For **v1.0.0** specifically, work top to bottom: the summary checklist below is
the release gate; the numbered sections that follow are the exact procedures.

---

## ✅ v1.0.0 release gate

### Build validation

- [ ] **Linux AppImage** builds and launches (`Nexo_1.0.0_amd64.AppImage`)
- [x] **Linux deb** builds; payload verified (binary + icons + `.desktop`) — install on a clean machine still pending
- [ ] **Windows MSI** builds and installs (`Nexo_1.0.0_x64_en-US.msi`)
- [ ] **Windows portable exe** launches by double-click (`Nexo-portable.exe`)

### Application

- [ ] **First launch onboarding** — Welcome → Device setup → Ready; does not
      reappear on second launch
- [ ] **Device naming** — the name entered in onboarding is what other devices see
- [ ] **Discovery** — a second device appears automatically on the same network
- [ ] **Trust flow** — trust a device; fingerprint + first-trusted shown;
      rename + remove-trust work
- [ ] **Send file** — pick a device, confirm, transfer starts after approval
- [ ] **Receive approval** — incoming dialog shows device/file/size; Accept
      completes, Reject leaves no file on disk
- [ ] **Background receiver** — window closed, device still receives from the tray
- [ ] **Notifications** — incoming/completed/failed notifications appear when the
      window is hidden

### Reliability

- [x] **1 GB transfer** completes with matching SHA-256 _(verified — Linux↔Linux)_
- [x] **5 GB transfer** completes with matching SHA-256 (no idle-timeout, no OOM)
      _(verified — 1280/1280 chunks, ~49 s)_
- [x] **Interrupted transfer** — kill the sender mid-transfer; checkpoint persists
      _(verified — 1 GB, checkpoint held at 137/256)_
- [x] **Resume** — restart the sender; it resumes and completes (only missing
      chunks re-sent) _(verified — re-sent 119 of 256, completed 256/256)_
- [x] **SHA verification** — the receiver's whole-file SHA-256 gate passes; a
      corrupted transfer is rejected, not silently written _(verified — every
      transfer above matched; reject test leaves no file)_

> These were run **Linux ↔ Linux** with the release binary (see "Validation
> results" below). The **Linux ↔ Windows** cross-platform pass still requires a
> Windows peer.

---

## Validation results (v1.0.0)

Recorded during the final cross-platform validation phase. **Legend:**
✅ verified · ⏳ pending operator (needs Windows host / GUI display / second
machine — could not be run in the Linux dev/CI environment).

### Automated & code quality

- ✅ `cargo clippy --workspace --all-targets -- -D warnings` — clean
- ✅ `npm run build` (tsc + vite) — clean
- ✅ `cargo test` — cli 23, desktop 28, storage 17, networking 8, engine 7,
  crypto 6, common 9 (real-QUIC cli + SQLite storage suites are
  parallelism-sensitive; pass in isolation / serial / at default parallelism)

### Real transfer testing (Linux ↔ Linux, over QUIC, release build)

| Case | Result | Detail |
|---|---|---|
| 1 MB | ✅ SHA-256 match | ~26 MB/s |
| 100 MB | ✅ SHA-256 match | ~118 MB/s |
| **5 GB** | ✅ SHA-256 match | 1280/1280 chunks, ~49 s, ~108 MB/s — **no idle timeout** |
| Resume after interrupt | ✅ SHA-256 match | 1 GB killed at 137/256 chunks → resumed from 137/256, re-sent **only** the missing 119 chunks, completed 256/256 |
| Sender approval | ✅ | gated send path exercised (`--auto-accept` for automation) |
| Receiver approval | ✅ | gated receive path exercised (`--auto-accept` for automation) |

- ⏳ **Linux ↔ Windows** transfer — needs a Windows peer; not run here.

### Linux packaging

- ✅ **.deb** builds and payload verified — `Package: nexo`, `Version: 1.0.0`,
  `Maintainer: Nexo`, correct WebKitGTK/appindicator deps; contains
  `usr/bin/nexo-desktop`, hicolor icons (32/128), and `Nexo.desktop`
  (`Name=Nexo`, `Exec=nexo-desktop`, `Terminal=false`).
- ⏳ **AppImage** — build config ready; not produced in this environment (the
  sandbox lacks the gdk-pixbuf loader tree linuxdeploy needs). CI builds it.
- ⏳ **Icon in launcher / tray / autostart / notifications** — GUI behaviors that
  need a desktop session. Autostart file-writing is unit-tested; tray + OS
  notifications are wired via standard Tauri APIs but need a live display to see.

### Windows

- ✅ MSI + portable-exe **configuration** verified (see
  `docs/windows-install-test.md`).
- ⏳ **MSI install / launch / uninstall**, **portable exe launch**, and
  **`scripts/windows-check.ps1`** run — all require a Windows machine. Recorded as
  PENDING in `docs/windows-install-test.md` for an operator to complete.

### Security flow

- ✅ Consent gates on both sides exercised in the transfer tests; certificate
  trust unchanged. Reject path leaves no output file (covered by
  `receiver_rejection_cancels_transfer_and_writes_no_file`).

---

## 0. Pre-flight

- [ ] Working tree clean (`git status`), on the release branch.
- [ ] Version is **1.0.0** and consistent across `VERSION`,
      `apps/desktop/package.json`, `apps/desktop/src-tauri/tauri.conf.json`,
      `apps/desktop/src-tauri/Cargo.toml`, and `crates/cli/Cargo.toml`.
      (`nexo --version` should print `nexo 1.0.0`.)
- [ ] Release notes drafted (`docs/release-notes-v1.0.0.md`).
- [ ] No stray large files (`git status` shows no `*.bin`, `target/`, temp files;
      `.gitignore` covers them).

## 1. Build & quality gates

From the repo root:

```bash
cargo test --workspace -- --test-threads=4   # bounded: real-QUIC/SQLite suites
cargo clippy --workspace --all-targets -- -D warnings
```

From `apps/desktop`:

```bash
npm ci
npm run build          # tsc -b + vite build
npm run typecheck      # tsc -b
cargo test -p nexo-desktop
```

- [ ] All tests green (run heavy CLI/QUIC suites at `--test-threads=4`, `=1`, or in
      isolation; the real-QUIC + SQLite tests can flake under full parallelism —
      they pass in isolation / at bounded parallelism).
- [ ] Clippy clean with `-D warnings`.
- [ ] Frontend builds with no type errors.
- [ ] Release binary optimization enabled (`[profile.release]` in the workspace
      `Cargo.toml`: `strip`, `lto = "thin"`, `codegen-units = 1`).

## 2. Package

Linux (priority):

```bash
cd apps/desktop
# AppImage tooling needs FUSE-less extract + a populated gdk-pixbuf loader dir:
APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri build -- --bundles appimage
npm run tauri build -- --bundles deb
```

Artifacts:

- [ ] AppImage → `target/release/bundle/appimage/Nexo_1.0.0_amd64.AppImage`
- [ ] deb → `target/release/bundle/deb/Nexo_1.0.0_amd64.deb`

Windows (on a Windows host with MSVC + WebView2 — see
`docs/windows-development.md`; run `scripts/windows-check.ps1` to verify the
environment first):

```powershell
npm run tauri build          # MSI + raw exe
```

- [ ] MSI → `target\release\bundle\msi\Nexo_1.0.0_x64_en-US.msi`
- [ ] Portable exe → `target\release\nexo-desktop.exe` (ship as `Nexo-portable.exe`)

> **CI note:** `.github/workflows/release.yml` builds all three artifacts on tag /
> manual dispatch (Linux AppImage+deb on `ubuntu-22.04`, Windows MSI on
> `windows-latest`) and uploads them as `Nexo-linux.AppImage`, `Nexo-linux.deb`,
> `Nexo-windows.msi`. Publishing a GitHub Release is still a manual step.
>
> **AppImage troubleshooting:** if `failed to run linuxdeploy`, ensure FUSE-less
> mode (`APPIMAGE_EXTRACT_AND_RUN=1`) and that
> `/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders` exists (`sudo pacman -S gdk-pixbuf2` /
> `apt install --reinstall libgdk-pixbuf-2.0-0 librsvg2-common`). The `.deb`
> target does not use linuxdeploy and always builds.

## 3. Install checks

Per artifact, on a clean machine/VM (see `docs/linux-install.md`,
`docs/windows-install.md`):

- [ ] **Install** — AppImage: `chmod +x` then run; deb: `apt install ./…deb`;
      MSI: run installer (SmartScreen → More info → Run anyway on unsigned builds).
- [ ] **Launch** — window opens; app icon + name (`Nexo`) correct in the
      launcher/dock/Start Menu.
- [ ] **Metadata** — MSI shows name `Nexo`, version `1.0.0`, publisher `Nexo`;
      Start Menu + Desktop shortcut + uninstall entry present.
- [ ] **First-run** — onboarding appears and does not reappear on second launch.
- [ ] **Tray** — tray icon present; menu shows Status / Open / Receiving toggle /
      Settings / Quit.
- [ ] **Window close → tray** — closing hides to tray (background on); Quit exits.
- [ ] **Autostart** — "Start on login" creates `~/.config/autostart/nexo.desktop`
      (Linux) / a `HKCU\…\Run` value (Windows); disabling removes it.

## 4. Functional checks (two machines / two instances)

- [ ] **Discover** — devices appear in the Devices screen.
- [ ] **Send** — pick a device → sender approval → receiver approval → transfer.
- [ ] **Receive** — incoming dialog (+ OS notification when hidden); Accept
      completes, Reject leaves no file.
- [ ] **Integrity** — received file SHA-256 matches the source.
- [ ] **1 GB and 5 GB** transfers complete end to end.
- [ ] **Resume** — interrupt a large transfer, reconnect, confirm resume +
      completion.
- [ ] **Trust** — trust a device; fingerprint + first-trusted shown; rename +
      remove-trust work.
- [ ] **History** — completed / cancelled / failed transfers are recorded.

## 5. Uninstall

- [ ] deb: `sudo apt remove nexo-desktop` removes the binary + desktop entry.
- [ ] AppImage: deleting the file removes the app; `~/.config/autostart/nexo.desktop`
      cleaned if autostart was enabled then disabled.
- [ ] Windows: Settings → Apps → Nexo → Uninstall.
- [ ] App data (`~/.local/share/dev.nexo.desktop` / `%APPDATA%\dev.nexo.desktop`)
      is intentionally **not** auto-removed; note this for users.

## 6. Publish

- [ ] Attach the three artifacts to a GitHub Release for tag `v1.0.0`.
- [ ] Paste `docs/release-notes-v1.0.0.md` as the release description.
- [ ] (Future) Sign artifacts + publish `latest.json` — see `docs/update-system.md`.
- [ ] Tag: `git tag v1.0.0 && git push origin v1.0.0` (triggers the release build).
- [ ] Smoke-test each published artifact on a clean machine.
