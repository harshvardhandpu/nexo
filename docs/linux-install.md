# Installing Nexo on Linux

Nexo ships as a portable **AppImage** and a **`.deb`** package. Both contain the
same application; pick whichever fits your distro.

- **Arch Linux / any distro** → AppImage (no install, just run)
- **Debian / Ubuntu / Mint / Pop!_OS** → `.deb`

---

## System requirements

Nexo uses WebKitGTK 4.1 and GTK 3 (via Tauri v2).

### Arch Linux

```bash
sudo pacman -S --needed webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg
```

The `.deb` is Debian-only. On Arch, use the **AppImage** (below) or build from
source.

### Debian / Ubuntu

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1
```

Tray icons need an app-indicator host: GNOME users should enable the
"AppIndicator and KStatusNotifierItem Support" extension; KDE/XFCE/Cinnamon have
it built in.

---

## AppImage (all distros)

```bash
chmod +x Nexo-linux.AppImage
./Nexo-linux.AppImage
```

That's it — the first launch shows onboarding. To integrate it into your app
menu (icon + `.desktop` entry), use `appimaged` or a tool like Gear Lever, or
copy a desktop entry manually:

```bash
mkdir -p ~/.local/bin ~/.local/share/applications
cp Nexo-linux.AppImage ~/.local/bin/nexo
cat > ~/.local/share/applications/nexo.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=Nexo
Comment=Encrypted peer-to-peer file transfer
Exec=/home/USER/.local/bin/nexo
Icon=nexo
Terminal=false
Categories=Utility;Network;FileTransfer;
EOF
```

> AppImage troubleshooting: if it fails to start with a FUSE error, either
> install FUSE 2 (`sudo pacman -S fuse2` / `sudo apt install libfuse2`) or run
> with `APPIMAGE_EXTRACT_AND_RUN=1 ./Nexo-linux.AppImage`.

---

## .deb (Debian / Ubuntu)

```bash
sudo apt install ./Nexo-linux.deb     # resolves dependencies
# or
sudo dpkg -i Nexo-linux.deb && sudo apt -f install
```

This installs:

- the binary at `/usr/bin/nexo-desktop`
- an app launcher at `/usr/share/applications/Nexo.desktop`
- hicolor icons under `/usr/share/icons/`

Launch from your app menu ("Nexo") or run `nexo-desktop`.

Uninstall:

```bash
sudo apt remove nexo-desktop      # or: sudo dpkg -r nexo-desktop
```

---

## What to verify after install

- [ ] App launches and shows onboarding on first run.
- [ ] Icon appears in the launcher/dock.
- [ ] Tray icon shows (Status / Open / Receiving / Settings / Quit).
- [ ] Closing the window hides to tray (with background mode on); Quit exits.
- [ ] "Start on login" adds `~/.config/autostart/nexo.desktop`.
- [ ] Incoming transfers raise a desktop notification when the window is hidden.
- [ ] A file sends to another device and the SHA-256 matches.

---

## Autostart & background mode

Enable **Settings → General → Start Nexo on login** to have Nexo launch (hidden,
into the tray) when you sign in. This writes a standard XDG autostart entry at
`~/.config/autostart/nexo.desktop`, honored on both X11 and Wayland. Disabling
the setting removes the entry.

**Background receiving** (on by default) keeps Nexo discoverable and able to
accept transfers after you close the window — it stays in the tray.

---

## Application data

Nexo stores its state under your XDG data dir, e.g.
`~/.local/share/dev.nexo.desktop/`:

- `state/` — transfer database, receiver identity, checkpoints
- `trusted-devices.json`, `transfer-history.json`, `preferences.json`
- `reports/` — exported benchmark reports

Removing the app does not delete this directory; delete it manually to reset.

---

## Building from source

```bash
git clone https://github.com/harshvardhandpu/nexo
cd nexo/apps/desktop
npm ci
npm run tauri build            # produces AppImage + deb under target/release/bundle
```

See `docs/release-checklist.md` for the full build/validate/package flow.
