import { Activity, Home, Radio, Send, Settings, Zap } from "lucide-react";
import type { NavItem } from "../components/Sidebar";

export type Screen =
  | "dashboard"
  | "send"
  | "receive"
  | "monitor"
  | "stress"
  | "settings";

export const NAV: ReadonlyArray<NavItem<Screen>> = [
  { id: "dashboard", label: "Dashboard", icon: Home },
  { id: "send", label: "Send File", icon: Send },
  { id: "receive", label: "Receive", icon: Radio },
  { id: "monitor", label: "Monitor", icon: Activity },
  { id: "stress", label: "Stress Mode", icon: Zap },
  { id: "settings", label: "Settings", icon: Settings },
];

export const TITLES: Record<Screen, { title: string; subtitle: string }> = {
  dashboard: { title: "Dashboard", subtitle: "Your Nexo transfer command center" },
  send: { title: "Send File", subtitle: "Drop a file to move it over encrypted QUIC" },
  receive: { title: "Receive", subtitle: "Advertise this device and discover peers" },
  monitor: { title: "Transfer Monitor", subtitle: "Live chunk-level transfer visualization" },
  stress: { title: "Stress Mode", subtitle: "Automated repeated large-file reliability runs" },
  settings: { title: "Settings", subtitle: "Storage locations and device identity" },
};
