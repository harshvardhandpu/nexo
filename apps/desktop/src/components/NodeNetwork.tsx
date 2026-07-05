/**
 * Decorative "connection" visualization: a constellation of peer nodes joined by
 * animated dashed edges with a glowing pulse (pure SVG + CSS, see theme.css).
 * `live` brightens it to signal an active/ready connection.
 */
type Node = { x: number; y: number; kind: "c" | "p" };

const NODES: Node[] = [
  { x: 40, y: 100, kind: "c" },
  { x: 120, y: 46, kind: "p" },
  { x: 128, y: 150, kind: "c" },
  { x: 200, y: 96, kind: "c" },
  { x: 270, y: 44, kind: "p" },
  { x: 284, y: 148, kind: "c" },
];

const EDGES: Array<[number, number]> = [
  [0, 1],
  [0, 2],
  [1, 3],
  [2, 3],
  [3, 4],
  [3, 5],
  [4, 5],
];

export function NodeNetwork({ live = false }: { live?: boolean }) {
  return (
    <svg
      className="nodes"
      viewBox="0 0 320 200"
      preserveAspectRatio="xMidYMid meet"
      style={{ opacity: live ? 1 : 0.65 }}
    >
      {EDGES.map(([a, b], index) => (
        <line
          key={`e${index}`}
          className="nodes__edge"
          x1={NODES[a].x}
          y1={NODES[a].y}
          x2={NODES[b].x}
          y2={NODES[b].y}
          style={{ animationDelay: `${index * -0.2}s` }}
        />
      ))}
      {NODES.map((node, index) => (
        <circle
          key={`n${index}`}
          className={`nodes__node ${node.kind === "p" ? "nodes__node--p" : ""}`}
          cx={node.x}
          cy={node.y}
          r={index === 3 ? 7 : 5}
          style={{ animationDelay: `${index * -0.5}s` }}
        />
      ))}
    </svg>
  );
}
