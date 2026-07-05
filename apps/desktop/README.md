# Nexo Desktop

A modern, animated desktop application for Nexo — a thin **UI layer** over the
unchanged Nexo core (QUIC networking, storage engine, resume system, transfer
protocol). The desktop app never reimplements engine logic; it only calls
existing core APIs through a small Rust/Tauri bridge.

- **Stack:** Tauri v2 · React 18 · TypeScript · Vite
- **Theme:** "Midnight Flow" — dark, glassmorphism, cyan → purple neon, pure-CSS
  animations (no runtime animation library).

---

## Architecture

```
┌────────────────────────────────────────────┐
│  React UI (src/)                            │
│  screens · components · useDesktopData hook │
└───────────────┬────────────────────────────┘
                │ @tauri-apps/api  invoke()/drag-drop
┌───────────────▼────────────────────────────┐
│  Rust bridge (src-tauri/src/lib.rs)         │
│  #[tauri::command] wrappers + job registry  │
└───────────────┬────────────────────────────┘
                │ calls only public core APIs
┌───────────────▼────────────────────────────┐
│  Nexo core crates  (UNMODIFIED)             │
│  cli · engine · networking · storage · crypto│
└─────────────────────────────────────────────┘
```

### Bridge commands (`src-tauri/src/lib.rs`)

| Command | Core API used |
|---|---|
| `get_settings`, `get_state_paths` | `CliConfig`, `CliStatePaths` |
| `get_status` | `cli::transfer_status_snapshot` |
| `get_receiver_endpoint` | `cli::receiver_endpoint` |
| `discover_known_peers` | `cli::discover_peers` |
| `start_send` / `start_receive` | `cli::run_send` / `cli::run_receive` |
| `list_transfer_jobs`, `reset_completed_jobs` | in-process job registry |
| `start_stress_run` | loops `cli::run_send` N times |
| `list_stress_runs`, `reset_completed_stress_runs` | stress registry |

Transfers run on background threads; progress is captured from the exact
progress lines the core already prints and parsed in the UI
(`src/lib/progress.ts`). No networking/QUIC/storage/resume code is touched.

---

## Screens

1. **Dashboard** — hero + animated connection node-network, live stats, latest transfer, active jobs.
2. **Send File** — native drag & drop (or paste a path), optional receiver address, live send activity.
3. **Receive** — advertise this device, LAN peer discovery, incoming activity.
4. **Transfer Monitor** — real-time chunk-grid visualization, liquid progress, throughput.
5. **Stress Mode** — automated *file × N* repeated transfers with per-iteration pass/fail grid.
6. **Settings** — storage locations, state files, device identity.

Animations (all pure CSS, see `src/styles/theme.css`): page fade+slide,
liquid-flow progress, success glow-burst, error shake + red pulse, connection
node glow.

---

## Prerequisites

- **Rust** (stable) + Cargo
- **Node.js** ≥ 18 and npm
- **Linux system libraries** (Tauri v2 / WebKitGTK):

  ```bash
  # Debian/Ubuntu
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev \
    libayatana-appindicator3-dev librsvg2-dev patchelf build-essential curl wget file

  # Arch
  sudo pacman -S webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg patchelf
  ```

Install JS dependencies once:

```bash
cd apps/desktop
npm install
```

---

## Develop

```bash
cd apps/desktop
npm run tauri dev        # launches the app with hot-reload
```

Frontend-only preview (no native window / bridge):

```bash
npm run dev              # http://127.0.0.1:1420
```

Checks:

```bash
npm run typecheck        # tsc project build (type-checks all TS)
npm run build            # tsc + vite production bundle -> dist/
cargo test -p nexo-desktop
```

---

## Build installers

Linux (priority — produces **AppImage** and **.deb**):

```bash
cd apps/desktop
npm run tauri build
# artifacts: src-tauri/target/release/bundle/appimage/*.AppImage
#            src-tauri/target/release/bundle/deb/*.deb
```

Restrict to a single target:

```bash
npm run tauri build -- --bundles appimage
npm run tauri build -- --bundles deb
```

### Linux bundling troubleshooting

- **`failed to run linuxdeploy` in a headless / container / no-FUSE shell** — the
  bundler's `linuxdeploy`/`appimagetool` are AppImages that need FUSE2 to mount.
  Extract-and-run instead:

  ```bash
  APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 npm run tauri build -- --bundles appimage
  ```

- **AppImage GTK plugin: `cp: cannot stat '.../gdk-pixbuf-2.0/2.10.0'`** — the
  gdk-pixbuf image-loader modules are not present at the path pkg-config
  advertises (some minimal images register the package but ship no loader files).
  Reinstall gdk-pixbuf so the loader tree exists:

  ```bash
  sudo pacman -S gdk-pixbuf2                              # Arch
  sudo apt install --reinstall libgdk-pixbuf-2.0-0 librsvg2-common   # Debian/Ubuntu
  ```

- The **`.deb` target does not use linuxdeploy** at all, so it always bundles
  even when the AppImage tooling can't run. It ships `usr/bin/nexo-desktop`, a
  `.desktop` launcher, and the icon — a reliable fallback installer.

Windows (secondary) — run on Windows with the MSVC toolchain + WebView2:

```powershell
npm run tauri build -- --bundles msi     # or: nsis
# artifact: src-tauri\target\release\bundle\msi\*.msi
```

> Icons: `src-tauri/icons/icon.png` is used for bundling. To regenerate the full
> platform icon set (including Windows `.ico`), run `npm run tauri icon path/to/logo.png`.

---

## Theme system

Design tokens live in `src/styles/theme.css` under `:root` (surfaces, neon
accents, radii, glow, motion easings). Retheme by editing those variables — the
whole UI (glass panels, buttons, progress, node glow) derives from them.
Component/layout styles are in `src/styles/app.css`.

---

## Integration guarantees

- No changes to `crates/networking`, `crates/storage`, `crates/crypto`,
  `crates/engine`, or the transfer protocol.
- The UI executes transfers **only** through existing core entry points via the
  Tauri bridge, so engine behavior (encryption, integrity, resume) is identical
  to the CLI.
