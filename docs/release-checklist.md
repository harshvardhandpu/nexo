# Nexo Release Checklist

A repeatable checklist for cutting a Nexo desktop release. The core engine
(networking/QUIC, storage, engine, crypto, resume, protocol) is frozen for
release-candidate work — only the desktop app + packaging change.

## 0. Pre-flight

- [ ] Working tree clean (`git status`), on the release branch.
- [ ] Version bumped in `apps/desktop/src-tauri/tauri.conf.json` (`version`) and
      `apps/desktop/package.json`.
- [ ] `CHANGELOG` / release notes drafted.

## 1. Build & quality gates

Run from the repo root:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

From `apps/desktop`:

```bash
npm ci
npm run build          # tsc -b + vite build
npm run typecheck      # tsc -b
cargo test -p nexo-desktop
```

- [ ] All tests green (run heavy CLI/QUIC suites at `--test-threads=4` or in
      isolation; the real-QUIC + SQLite tests can flake under full parallelism —
      see notes in the repo).
- [ ] Clippy clean with `-D warnings`.
- [ ] Frontend builds with no type errors.

## 2. Package

Linux (priority):

```bash
cd apps/desktop
# AppImage tooling needs FUSE-less extract + a populated gdk-pixbuf loader dir:
APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri build -- --bundles appimage
npm run tauri build -- --bundles deb
```

Artifacts:

- [ ] AppImage → `target/release/bundle/appimage/Nexo_<ver>_amd64.AppImage`
- [ ] deb → `target/release/bundle/deb/Nexo_<ver>_amd64.deb`

Windows (secondary, on a Windows host with MSVC + WebView2):

```bash
npm run tauri build -- --bundles msi   # or: nsis
```

- [ ] MSI/NSIS → `src-tauri\target\release\bundle\msi\Nexo_<ver>_x64_en-US.msi`

> AppImage troubleshooting: if `failed to run linuxdeploy`, ensure FUSE-less mode
> (`APPIMAGE_EXTRACT_AND_RUN=1`) and that `/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders`
> exists (`sudo pacman -S gdk-pixbuf2` / `apt install --reinstall
> libgdk-pixbuf-2.0-0 librsvg2-common`). The `.deb` target does not use
> linuxdeploy and always builds.

## 3. Install checks

Per artifact, on a clean machine/VM:

- [ ] **Install** — AppImage: `chmod +x` then run directly; deb: `dpkg -i`
      (or `apt install ./Nexo_<ver>_amd64.deb`); MSI: run installer.
- [ ] **Launch** — window opens; app icon + name correct in the launcher/dock.
- [ ] **First-run** — onboarding appears (Welcome → Device setup → Ready) and
      does **not** reappear on the second launch.
- [ ] **Tray** — tray icon present; menu shows Status / Open / Receiving toggle /
      Settings / Quit; left-click opens the menu.
- [ ] **Window close → tray** — closing the window hides to tray (background on)
      and the app keeps running; Quit from tray fully exits.
- [ ] **Background receiver** — with background mode on and the window closed,
      the device is still discoverable from another machine.
- [ ] **Autostart** — enabling "Start on login" creates
      `~/.config/autostart/nexo.desktop` (Linux) / a `HKCU\…\Run` value
      (Windows); disabling removes it.

## 4. Functional checks (two machines / two instances)

- [ ] **Discover** — devices appear in the Devices screen.
- [ ] **Send** — pick a device → sender approval → receiver approval → transfer.
- [ ] **Receive** — incoming request shows a dialog (and an OS notification when
      hidden); Accept completes, Reject leaves no file.
- [ ] **Integrity** — received file SHA-256 matches the source.
- [ ] **Resume** — interrupt a large transfer, reconnect, confirm it resumes and
      completes.
- [ ] **Trust** — trust a device; fingerprint + first-trusted shown; rename +
      remove-trust work.
- [ ] **History** — completed / cancelled / failed transfers are recorded.

## 5. Uninstall

- [ ] deb: `apt remove nexo` / `dpkg -r nexo` removes the binary + desktop entry.
- [ ] AppImage: deleting the file removes the app; `~/.config/autostart/nexo.desktop`
      cleaned if autostart was enabled and later disabled.
- [ ] App data (`~/.local/share/dev.nexo.desktop` or platform equivalent) noted
      for the user (not auto-removed by design).

## 6. Publish

- [ ] Sign artifacts (see `docs/update-system.md` → Signed releases).
- [ ] Upload artifacts + `latest.json` manifest.
- [ ] Tag the release (`git tag vX.Y.Z`), push, attach artifacts.
- [ ] Smoke-test the published manifest against a running client.
