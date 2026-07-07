import { getCurrentWindow } from "@tauri-apps/api/window";
import type { IncomingTransfer } from "../api/desktop";
import { formatBytes } from "../utils";

/**
 * Feature 4: when an incoming transfer arrives while the window is hidden, show
 * an OS notification (browser Notification API — no extra Tauri plugin/dep).
 * Clicking it reveals the window so the already-open approval dialog is visible.
 * When the window is already visible, the in-app dialog is enough, so we skip
 * the OS notification.
 */
export async function notifyIncoming(request: IncomingTransfer): Promise<void> {
  let hidden = false;
  try {
    hidden = !(await getCurrentWindow().isVisible());
  } catch {
    hidden = false;
  }
  if (!hidden || typeof Notification === "undefined") {
    return;
  }

  const show = () => {
    const notification = new Notification("Nexo — incoming file", {
      body: `From ${request.sender}\n${request.filename} · ${formatBytes(
        request.fileSize,
      )}\nClick to review`,
    });
    notification.onclick = () => {
      void getCurrentWindow().show();
      void getCurrentWindow().setFocus();
    };
  };

  if (Notification.permission === "granted") {
    show();
  } else if (Notification.permission !== "denied") {
    try {
      const permission = await Notification.requestPermission();
      if (permission === "granted") {
        show();
      }
    } catch {
      /* notifications unavailable: the in-app dialog still appears on open */
    }
  }
}
