# Windows Installation Test Record — Nexo v1.0.0

> **Status: PENDING operator execution on a Windows machine.**
>
> This is the test record for Nexo's Windows installer and portable executable.
> The steps and expected results below are derived from the packaging
> configuration (`apps/desktop/src-tauri/tauri.conf.json`) and the Tauri MSI
> defaults, and were **validated at the config/build level** — but the actual
> install, launch, and uninstall on Windows **must be run by an operator on a
> real Windows 10/11 machine** and the results filled in. It has not been
> executed in the Linux CI/dev environment where these builds are prepared.
>
> Fill in the `Result` column with `PASS` / `FAIL` + notes, attach screenshots,
> and record the environment. Then mirror the outcomes into
> `docs/release-checklist.md`.

## Test environment

| Field | Value |
|---|---|
| Windows version | _e.g. Windows 11 23H2 (build 22631)_ · **TBD** |
| Architecture | x64 |
| WebView2 present before install? | _yes / no_ · **TBD** |
| Installed from | `Nexo_1.0.0_x64_en-US.msi` (built by `.github/workflows/release.yml`) |
| Tester / date | **TBD** |

## Part 1 — MSI installation

Artifact: `Nexo-windows.msi` (a.k.a. `Nexo_1.0.0_x64_en-US.msi`).

| # | Step | Expected | Result |
|---|---|---|---|
| 1 | Double-click the MSI | Installer opens; on unsigned build, SmartScreen "More info → Run anyway" | **PENDING** |
| 2 | Complete the install | Installs to `C:\Program Files\Nexo\`; `Nexo.exe` present | **PENDING** |
| 3 | Start Menu | **Nexo** entry appears | **PENDING** |
| 4 | Desktop shortcut | Desktop shortcut created | **PENDING** |
| 5 | Launch | App window opens; no console window | **PENDING** |
| 6 | Icon | Correct Nexo icon in taskbar / Start Menu / window | **PENDING** |
| 7 | Version metadata | Settings → About shows **Nexo 1.0.0**; Apps & features shows version 1.0.0, publisher **Nexo** | **PENDING** |
| 8 | WebView2 auto-install | On a machine without WebView2, the installer fetches + installs it (needs internet) | **PENDING** |
| 9 | Uninstall | Settings → Apps → Nexo → Uninstall removes the app, shortcuts, and Start Menu entry | **PENDING** |

**Config verification (done, Linux side):** `productName: "Nexo"` → `Nexo.exe`;
`bundle.targets` includes `msi`; `icon.ico` bundled; `publisher: "Nexo"`,
`version: "1.0.0"`; `windows.webviewInstallMode: downloadBootstrapper` (auto WebView2);
`wix.language: en-US` → `Nexo_1.0.0_x64_en-US.msi`. Start Menu + Desktop shortcut +
uninstall entry are Tauri/WiX MSI defaults.

## Part 2 — Portable executable

Artifact: `Nexo-portable.exe` (the raw `target\release\nexo-desktop.exe`).

| # | Step | Expected | Result |
|---|---|---|---|
| 1 | Double-click `Nexo-portable.exe` | App launches directly | **PENDING** |
| 2 | No terminal | No console/terminal window appears | **PENDING** |
| 3 | No Rust required | Runs on a machine with no Rust toolchain | **PENDING** |
| 4 | No Node required | Runs on a machine with no Node.js | **PENDING** |
| 5 | WebView2 only dependency | Launches when WebView2 is present; fails gracefully / prompts if absent | **PENDING** |
| 6 | Data location | Stores state under `%APPDATA%\dev.nexo.desktop` | **PENDING** |

## Screenshots

Attach to `docs/screenshots/` and link here:

- [ ] `windows-installer.png` — the MSI install dialog
- [ ] `windows-startmenu.png` — Nexo in the Start Menu
- [ ] `windows-running.png` — the app running on Windows
- [ ] `windows-uninstall.png` — the Apps & features / uninstall entry

## Known issues / notes

- **Unsigned builds:** SmartScreen and some antivirus will warn (expected until
  code signing is added). Documented in `docs/windows-install.md`.
- Record any deviations from the expected results here (installer errors, missing
  shortcut, blank window = WebView2 missing, firewall prompt behavior, etc.).

---

_When this record is completed with real PASS/FAIL results and screenshots, update
`docs/release-checklist.md` (Windows MSI tested / Portable exe tested) and, if
needed, `docs/windows-install.md`._
