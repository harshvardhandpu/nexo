# Nexo Screenshots

Drop the app screenshots here so they render in the main [`README.md`](../../README.md).
Capture them on a machine with a display (the app is a desktop GUI).

Expected files (PNG, ~1200–1600 px wide, dark theme):

| File | Screen | What to show |
|---|---|---|
| `dashboard.png` | Dashboard | Home view: receiver status ("Available"), stats, node-network animation |
| `send.png` | Send | Drag-and-drop send screen with a file selected |
| `receive-approval.png` | Receive approval | The incoming-transfer dialog (device, file, size, Accept / Reject) |
| `transfer-progress.png` | Transfer monitor | A live transfer: liquid progress bar + chunk grid |
| `settings.png` | Settings | Sectioned settings (General / Transfer / Privacy) |

## How to capture

1. Build and run the app: `cd apps/desktop && npm run tauri dev`.
2. Complete onboarding once so the main UI is visible.
3. Use your OS screenshot tool (e.g. `gnome-screenshot -w`, Spectacle, or
   `Win + Shift + S`) to grab each window.
4. Save with the exact filenames above so the README image links resolve.

> Until real screenshots are added, the README links point to these paths and
> will show broken-image placeholders — replace them with actual PNGs before a
> public release.
