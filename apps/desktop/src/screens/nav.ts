import {
  Activity,
  Clock,
  Home,
  MonitorSmartphone,
  Radio,
  Send,
  Settings,
  ShieldCheck,
  Zap,
} from "lucide-react";
import type { NavItem } from "../components/Sidebar";

export type Screen =
  | "dashboard"
  | "devices"
  | "send"
  | "receive"
  | "monitor"
  | "history"
  | "trusted"
  | "stress"
  | "settings";

export const NAV: ReadonlyArray<NavItem<Screen>> = [
  { id: "dashboard", label: "Dashboard", icon: Home },
  { id: "devices", label: "Devices", icon: MonitorSmartphone },
  { id: "send", label: "Send File", icon: Send },
  { id: "receive", label: "Receive", icon: Radio },
  { id: "monitor", label: "Monitor", icon: Activity },
  { id: "history", label: "History", icon: Clock },
  { id: "trusted", label: "Trusted", icon: ShieldCheck },
  { id: "stress", label: "Stress Mode", icon: Zap },
  { id: "settings", label: "Settings", icon: Settings },
];

export const TITLES: Record<Screen, { title: string; subtitle: string }> = {
  dashboard: { title: "Dashboard", subtitle: "Your Nexo transfer command center" },
  devices: {
    title: "Devices",
    subtitle: "Peers on your network, live presence and trust",
  },
  send: { title: "Send File", subtitle: "Drop a file to move it over encrypted QUIC" },
  receive: { title: "Receive", subtitle: "Advertise this device and discover peers" },
  monitor: { title: "Transfer Monitor", subtitle: "Live chunk-level transfer visualization" },
  history: {
    title: "Transfer History",
    subtitle: "Every transfer, with status and integrity result",
  },
  trusted: {
    title: "Trusted Devices",
    subtitle: "Manage certificate trust and device names",
  },
  stress: { title: "Stress Mode", subtitle: "Automated repeated large-file reliability runs" },
  settings: { title: "Settings", subtitle: "Storage locations and device identity" },
};
