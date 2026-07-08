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
- [ ] **Linux deb** builds and installs (`Nexo_1.0.0_amd64.deb`)
- [ ] **Windows MSI** builds and installs (`Nexo_1.0.0_x64_en-US.msi`)

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

- [ ] **1 GB transfer** completes with matching SHA-256
- [ ] **5 GB transfer** completes with matching SHA-256 (no idle-timeout, no OOM)
- [ ] **Interrupted transfer** — kill the sender mid-transfer; checkpoint persists
- [ ] **Resume** — restart the sender; it resumes and completes (only missing
      chunks re-sent)
- [ ] **SHA verification** — the receiver's whole-file SHA-256 gate passes; a
      corrupted transfer is rejected, not silently written

> Automated coverage for the reliability row already exists:
> `cargo test -p cli` includes real interrupt→resume→verify tests, and
> `crash_recovery_reliability_report_10x` (run with `--ignored`) reports a 10×
> interrupt/resume/verify success rate. The 5 GB run is the manual, full-scale
> confirmation.

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

Windows (on a Windows host with MSVC + WebView2):

```bash
npm run tauri build -- --bundles msi
```

- [ ] MSI → `target\release\bundle\msi\Nexo_1.0.0_x64_en-US.msi`

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
