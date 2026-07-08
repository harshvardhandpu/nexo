# Installing Nexo on Windows

Nexo ships as a standard Windows **MSI installer**. This guide covers install,
first use, and troubleshooting for a normal Windows 10 / 11 user.

---

## Requirements

- Windows 10 (64-bit) or Windows 11.
- **WebView2 runtime** — already present on Windows 11 and up-to-date Windows 10.
  If it's missing, the Nexo installer downloads and installs it automatically, so
  the first install needs an internet connection.

---

## Installation

1. **Download the MSI**
   Get `Nexo-windows.msi` from the
   [latest release](https://github.com/harshvardhandpu/nexo/releases).

2. **Run the installer**
   Double-click `Nexo-windows.msi` and follow the prompts.
   - Because the installer isn't code-signed yet, Windows **SmartScreen** may
     show *"Windows protected your PC."* Click **More info → Run anyway**. (See
     [Antivirus / SmartScreen](#antivirus--smartscreen-false-positives) below.)
   - Nexo installs to `C:\Program Files\Nexo\`, adds a **Start Menu** entry and a
     **Desktop shortcut**, and registers an **uninstall** entry in
     *Settings → Apps → Installed apps*.

3. **Launch Nexo**
   Open **Nexo** from the Start Menu or the desktop shortcut.

4. **Complete onboarding**
   On first launch, Nexo walks you through:
   - naming this device (how it appears to others),
   - choosing whether it's **discoverable** and receives **in the background**,
   - an optional **Test connection** check (storage, receiver, discovery),
   - then you're ready.

To confirm the installed version, open **Settings → About** (shows version,
build type, and commit).

### Portable (no installer)

If you'd rather not install anything, download **`Nexo-portable.exe`** from the
release, put it wherever you like, and **double-click it** — Nexo launches
directly. No terminal, no Rust, no Node; it only needs the **WebView2 runtime**
(see [Requirements](#requirements)). The portable exe doesn't create Start Menu /
desktop shortcuts, but it's otherwise the same app and stores its data in the
same place (`%APPDATA%\dev.nexo.desktop`).

> Prefer the **MSI** for everyday use (shortcuts, uninstall entry, automatic
> WebView2 install). Use the portable exe for a quick try or a locked-down
> machine where you can't run installers.

---

## Using Nexo

### Send a file

1. Open **Send** (or drag a file onto the window).
2. Pick a discovered device, or paste its address.
3. Confirm the send. The other device must **accept** before any data moves.

### Receive a file

- With **background receiving** on (default), Nexo is ready to receive whenever
  it's running — even minimized to the tray.
- When a file comes in, Nexo shows an **approval dialog** (and a desktop
  notification if the window is hidden). Click **Accept** to receive or
  **Reject** to decline. Nothing is written to disk until you accept.
- Received files land in your chosen download folder (Settings → Transfer), or
  the default location otherwise.

### Background mode & the system tray

- Closing the window **minimizes Nexo to the system tray** (near the clock) so it
  keeps receiving. Right-click the tray icon for **Open**, the receiving toggle,
  **Settings**, and **Quit**.
- To fully exit, choose **Quit** from the tray menu.
- **Start on login:** enable *Settings → General → Start Nexo on login* to launch
  Nexo automatically (into the tray) when you sign in.

> Tray tip: if you don't see the icon, click the **^** ("Show hidden icons")
> arrow on the taskbar and drag Nexo's icon onto the taskbar to keep it visible.

### Trusted devices

- After a successful transfer you can **trust** a device (Devices screen). Nexo
  shows its certificate **fingerprint** and when you first trusted it.
- Trusting is optional and never bypasses encryption. If you enable
  *auto-accept from trusted devices* (Settings → Transfer), transfers from those
  devices skip the approval prompt — everything else still asks.

---

## Troubleshooting

### Firewall permissions

Nexo transfers files directly between devices over the network, so Windows
Defender Firewall must allow it.

- The **first time** Nexo needs the network, Windows shows a
  *"Windows Defender Firewall has blocked some features"* dialog.
  **Check "Private networks"** and click **Allow access**. (Home/work Wi-Fi and
  Ethernet are "Private"; leave "Public networks" unchecked unless you know you
  need it.)
- If you dismissed it by accident: **Settings → Privacy & security → Windows
  Security → Firewall & network protection → Allow an app through firewall**, find
  **Nexo**, and enable it for **Private**.

### Devices don't appear (network discovery)

Nexo finds nearby devices with mDNS on the local network. If a device you expect
isn't listed:

- Make sure **both devices are on the same network** (same Wi-Fi / same router).
  Guest networks and "client isolation" / "AP isolation" on the router block
  device-to-device traffic — turn AP isolation off or use a normal network.
- Confirm your network is set to **Private**, not **Public**
  (*Settings → Network & internet → Wi-Fi → your network → Private*). Windows
  blocks discovery on Public networks.
- Allow Nexo through the firewall on **Private** networks (above).
- You can always transfer **without discovery** by entering the other device's
  address directly in the Send screen — discovery is a convenience, not a
  requirement.

### Antivirus / SmartScreen false positives

Because the current builds are **not yet code-signed**, Windows SmartScreen and
some antivirus tools may flag the installer or the app as "unknown."

- SmartScreen: **More info → Run anyway**.
- Antivirus: this is a false positive from the missing signature, not from the
  app's behavior. If your AV quarantines Nexo, restore it and add an exclusion
  for `C:\Program Files\Nexo\`. Only do this for builds you downloaded from the
  official releases page.
- Code signing is planned for a future release, which removes these warnings.

### Running behind a VPN

A VPN can interfere with **local** device discovery and direct connections:

- Many VPN clients route or block LAN traffic. If discovery fails while connected,
  enable your VPN's **"allow local (LAN) access"** option, or briefly disconnect
  the VPN for local transfers.
- For transfers between two devices on the **same** VPN, discovery may not work
  even though a direct connection would; use the **address** field on the Send
  screen instead.
- Nexo 1.0 is designed for local-network transfers. Cross-internet transfers
  through NAT/VPN are part of the future roadmap
  (see `docs/roadmap/nexo-2-share-links.md`), not 1.0.

### Nothing happens when I close the window

That's expected — Nexo **minimizes to the tray** so it can keep receiving. Use
the tray icon's **Quit** to exit completely, or turn off background receiving in
*Settings → General* if you'd rather closing the window exit the app.

---

## Uninstalling

**Settings → Apps → Installed apps → Nexo → Uninstall** (or *Control Panel →
Programs and Features*).

Your Nexo data (trusted devices, history, preferences) is stored under
`%APPDATA%\dev.nexo.desktop\` and is **not** removed by uninstalling. Delete that
folder manually if you want a clean reset.

---

## Building the MSI from source

On a Windows machine with the **MSVC** toolchain, **Node ≥ 18**, and **WebView2**:

```powershell
git clone https://github.com/harshvardhandpu/nexo
cd nexo\apps\desktop
npm install
npm run tauri build            # MSI  -> target\release\bundle\msi\
                               # exe  -> target\release\nexo-desktop.exe
```

For the full developer setup (Git, Rust MSVC, Node, Visual Studio C++ build
tools) and troubleshooting, see **[docs/windows-development.md](windows-development.md)**.
Run **`scripts/windows-check.ps1`** first to verify your environment.

See `docs/release-checklist.md` for the full release process.
