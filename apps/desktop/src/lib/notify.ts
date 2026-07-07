import { getCurrentWindow } from "@tauri-apps/api/window";
import type { IncomingTransfer } from "../api/desktop";
import { formatBytes } from "../utils";

/** Whether the app window is currently hidden (best-effort). */
async function windowHidden(): Promise<boolean> {
  try {
    return !(await getCurrentWindow().isVisible());
  } catch {
    return false;
  }
}

/** Fires a native notification, requesting permission on first use. */
async function fire(
  title: string,
  body: string,
  onClick?: () => void,
): Promise<void> {
  if (typeof Notification === "undefined") {
    return;
  }
  const show = () => {
    const notification = new Notification(title, { body });
    if (onClick) {
      notification.onclick = onClick;
    }
  };
  if (Notification.permission === "granted") {
    show();
  } else if (Notification.permission !== "denied") {
    try {
      if ((await Notification.requestPermission()) === "granted") {
        show();
      }
    } catch {
      /* notifications unavailable — the in-app UI still covers the flow */
    }
  }
}

/**
 * Task 4: incoming-transfer notification, shown only when the window is hidden
 * (in the tray). Clicking it reveals the window and the approval dialog.
 * Suppressed when the user disabled notifications.
 */
export async function notifyIncoming(
  request: IncomingTransfer,
  enabled: boolean,
): Promise<void> {
  if (!enabled || !(await windowHidden())) {
    return;
  }
  await fire(
    "Nexo",
    `${request.sender} wants to send ${request.filename} (${formatBytes(
      request.fileSize,
    )})`,
    () => {
      void getCurrentWindow().show();
      void getCurrentWindow().setFocus();
    },
  );
}

/** Task 4: transfer-completed notification (only when hidden). */
export async function notifyCompleted(
  filename: string,
  enabled: boolean,
): Promise<void> {
  if (!enabled || !(await windowHidden())) {
    return;
  }
  await fire("Transfer completed", `${filename} received successfully`);
}

/** Task 4: transfer-failed notification (only when hidden). */
export async function notifyFailed(
  filename: string,
  enabled: boolean,
): Promise<void> {
  if (!enabled || !(await windowHidden())) {
    return;
  }
  await fire("Transfer failed", `${filename} — resume available`);
}
