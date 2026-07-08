# Nexo Screenshots

Drop the app screenshots here so they render in the main [`README.md`](../../README.md)
and the release notes. Capture them on a machine with a display (the app is a
desktop GUI).

## Required screenshots (v1.0.0)

Provide all eight, as **PNG**, dark theme, ~1200–1600 px wide (the Windows
installer shot can be its native size):

| # | File | Screen | What to show |
|---|---|---|---|
| 1 | `dashboard.png` | Dashboard | Home view: receiver status ("Available"), stats, node-network animation |
| 2 | `send.png` | Send screen | Drag-and-drop send screen with a file selected |
| 3 | `discovery.png` | Device discovery | Devices screen listing at least one discovered device (online indicator + trust status) |
| 4 | `receive-approval.png` | Receiver approval | Incoming-transfer dialog (device, file, size, Accept / Reject) |
| 5 | `transfer-progress.png` | Transfer progress | A live transfer: liquid progress bar + chunk grid |
| 6 | `transfer-complete.png` | Completed transfer | A finished transfer (success state / history entry with SHA-256 verified) |
| 7 | `settings.png` | Settings | Sectioned settings (General / Transfer / Privacy) |
| 8 | `windows-installer.png` | Windows installer | The MSI install dialog (or Start Menu entry) on Windows |

## How to capture

1. Build and run the app: `cd apps/desktop && npm run tauri dev`.
2. Complete onboarding once so the main UI is visible.
3. Get a second device (or a second instance on another machine) online so
   **discovery** (#3), **receiver approval** (#4), **progress** (#5), and
   **completed** (#6) can be captured with a real transfer.
4. Use your OS screenshot tool:
   - Linux: `gnome-screenshot -w`, Spectacle, or your compositor's shortcut.
   - Windows: `Win + Shift + S` (for #8, capture the running MSI installer).
5. Save with the **exact filenames** above so the README/release-notes image
   links resolve.

## Consistency tips

- Same window size and theme across shots (the app defaults to the dark
  "Midnight Flow" theme).
- Use a realistic but non-sensitive file name (e.g. `holiday-photos.zip`).
- Avoid capturing personal IPs/hostnames you don't want public; the diagnostics
  and endpoint fields show real addresses.

> Until real screenshots are added, the README/release-notes image links point to
> these paths and will show broken-image placeholders. Replace them with actual
> PNGs before the public release.
