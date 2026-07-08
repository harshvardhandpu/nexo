# Building Nexo from source on Windows

This guide is for **developers** who want to build Nexo on Windows. Normal users
do **not** need any of this — they just download the installer (see
[docs/windows-install.md](windows-install.md)).

Building Nexo needs a working Rust MSVC toolchain, Node.js, and the Visual Studio
C++ build tools (Tauri compiles a native Windows app). Budget ~30 minutes for a
first-time setup, mostly downloads.

> **Shortcut:** after installing the prerequisites below, run
> [`scripts/windows-check.ps1`](../scripts/windows-check.ps1) from the repo root
> in PowerShell — it verifies every requirement and tells you exactly what's
> missing.

---

## Requirements

### Git

Install Git for Windows: <https://git-scm.com/download/win> (or `winget install
Git.Git`). Verify:

```powershell
git --version
```

### Rust

Install with rustup: <https://rustup.rs> (or `winget install Rustlang.Rustup`).
Choose the default, which installs the **MSVC** toolchain. Verify:

```powershell
rustc --version
cargo --version
```

Nexo must build with the MSVC toolchain (not GNU). Confirm/set it:

```powershell
rustup default stable-x86_64-pc-windows-msvc
rustup show          # "Default host: x86_64-pc-windows-msvc"
```

### Node.js

Install the **LTS** release: <https://nodejs.org> (or `winget install
OpenJS.NodeJS.LTS`). Verify:

```powershell
node --version       # v18 or newer
npm --version
```

### Visual Studio Build Tools

Rust's MSVC toolchain needs the Microsoft C++ linker and Windows SDK. Install
**Visual Studio Build Tools 2022**:

- Download: <https://visualstudio.microsoft.com/downloads/> →
  *Tools for Visual Studio* → **Build Tools for Visual Studio 2022**
  (or `winget install Microsoft.VisualStudio.2022.BuildTools`).
- In the installer, select the workload:

  **✅ Desktop development with C++**

  Make sure these components are included (they are, by default, in that
  workload):

  - **MSVC v143 – VS 2022 C++ x64/x86 build tools** (the compiler + `link.exe`)
  - **Windows 11 SDK** (or Windows 10 SDK)
  - **C++ CMake tools for Windows**

If you skip this, `cargo build` fails at the link step with:

```
error: linker `link.exe` not found
```

The fix is always: install the **Desktop development with C++** workload above.

### WebView2 runtime

Nexo's UI runs in **WebView2** (the same engine as Microsoft Edge).

- Windows 11 and up-to-date Windows 10 already have it.
- If it's missing, install the **Evergreen** runtime:
  <https://developer.microsoft.com/microsoft-edge/webview2/> (Evergreen
  Standalone Installer), or `winget install Microsoft.EdgeWebView2Runtime`.
- The **shipped MSI installs WebView2 automatically** if it's absent; you only
  need to install it manually for a `tauri dev` run on a machine that lacks it.

---

## Tauri requirements (summary)

Nexo's desktop app is built with [Tauri](https://tauri.app). On Windows it needs:

- **Rust MSVC toolchain** — `stable-x86_64-pc-windows-msvc`
- **Visual Studio C++ build tools** — MSVC compiler + Windows SDK
- **Node.js** — for the frontend build
- **WebView2** — to run the app

Verify the toolchain in one line:

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

---

## Build steps

Clone the repository and build the desktop app:

```powershell
git clone https://github.com/harshvardhandpu/nexo
cd nexo\apps\desktop

npm install            # install frontend dependencies

npm run tauri dev      # run the app with hot-reload (development)
```

For a production build (produces the installer + the raw executable):

```powershell
npm run tauri build
```

### Where the build output lands

After `npm run tauri build`, from the repo root:

| Output | Path |
|---|---|
| **MSI installer** | `target\release\bundle\msi\Nexo_1.0.0_x64_en-US.msi` |
| **Portable executable** | `target\release\nexo-desktop.exe` (rename to `Nexo.exe` to distribute) |

> Note: Cargo's `target\` directory is at the **workspace root**
> (`nexo\target\...`), not inside `apps\desktop`, because Nexo is a Cargo
> workspace.

You can also build just the Rust/CLI side from the repo root:

```powershell
cargo build --release            # builds the whole workspace
cargo run -p cli -- --version    # nexo 1.0.0
```

---

## Troubleshooting

### `error: linker `link.exe` not found`

**Cause:** the Visual Studio C++ build tools aren't installed (or not on PATH).

**Fix:** install **Build Tools for Visual Studio 2022** with the
**Desktop development with C++** workload (MSVC compiler, Windows SDK, C++ CMake
tools). Then open a **new** terminal and rebuild.

### `cargo build` fails on Windows

**Check the active toolchain:**

```powershell
rustup show
```

The default host should be:

```
stable-x86_64-pc-windows-msvc
```

If it shows `-gnu`, switch to MSVC:

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

### WebView2 missing

The app window is blank or fails to start, or `tauri dev` complains about
WebView2.

**Fix:** install the **Microsoft Edge WebView2 Runtime**
(<https://developer.microsoft.com/microsoft-edge/webview2/> or
`winget install Microsoft.EdgeWebView2Runtime`). The MSI installer handles this
automatically for end users.

### Windows Firewall blocks Nexo

Transfers and discovery use the local network, so Windows Defender Firewall must
allow Nexo.

**Fix:** when prompted on first network use, **check "Private networks"** and
click **Allow access**. To fix it later: *Windows Security → Firewall & network
protection → Allow an app through firewall → Nexo → enable Private*.

### `npm install` or `tauri` command errors

- Ensure Node **LTS** (`node --version` ≥ 18) and run `npm install` **inside**
  `apps\desktop`.
- Run `npm run tauri --version` to confirm the Tauri CLI is available (it's a dev
  dependency installed by `npm install`).

---

## Related docs

- [Install Nexo on Windows (users)](windows-install.md)
- [Release checklist](release-checklist.md)
- [Linux build & install](linux-install.md)
