import type { LucideIcon } from "lucide-react";

export type NavItem<T extends string> = {
  id: T;
  label: string;
  icon: LucideIcon;
};

export function Sidebar<T extends string>({
  items,
  active,
  onSelect,
}: {
  items: ReadonlyArray<NavItem<T>>;
  active: T;
  onSelect: (id: T) => void;
}) {
  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <div className="brand-mark">N</div>
        <div>
          <strong>Nexo</strong>
          <span>Midnight Flow</span>
        </div>
      </div>

      <nav className="sidebar__nav">
        {items.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              className={`nav-item ${active === item.id ? "active" : ""}`}
              onClick={() => onSelect(item.id)}
            >
              <Icon size={18} />
              {item.label}
            </button>
          );
        })}
      </nav>

      <div className="sidebar__foot">
        Encrypted QUIC transfer
        <br />
        with crash-safe resume.
      </div>
    </aside>
  );
}
