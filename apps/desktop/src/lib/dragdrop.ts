import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";

/**
 * Subscribes to the Tauri webview's native drag-drop events (built into
 * @tauri-apps/api, no extra plugin) and reports real absolute file paths on
 * drop. Returns whether a drag is currently hovering the window.
 */
export function useFileDrop(onDrop: (paths: string[]) => void): boolean {
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragging(true);
        } else if (payload.type === "leave") {
          setDragging(false);
        } else if (payload.type === "drop") {
          setDragging(false);
          if (payload.paths.length > 0) {
            onDrop(payload.paths);
          }
        }
      })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        /* running outside a Tauri window (e.g. plain vite preview): ignore */
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [onDrop]);

  return dragging;
}
