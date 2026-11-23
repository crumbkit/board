import { useState } from "react";
import { type Mark, type Side, OFFSET, RADIUS, labelBox, lineEnds, lineNote, shade } from "./mapGeometry";

type Item = {
  id: string;
  region?: string;
  label: string[];
  radius?: number;
  at?: { x: number; y: number; set: "engine" | "pinned" };
  side?: Side;
  bin?: number;
};
type Anchor = { item: { id: string; dx: number; dy: number } } | { line: { from: string; to: string; along: number; offset?: number } };
type Table = {
  id: string;
  title: string;
  caption?: string;
  columns: { label: string; numeric?: boolean }[];
  rows: { item?: string; cells: string[] }[];
};

/** A `map/layout/v1` record as Board stores it. */
export type MapData = {
  $type: string;
  title: string;
  sheet: { width: number; height: number };
  regions?: { id: string; title: string; x: number; y: number; width: number; height: number }[];
  items: Item[];
  lines?: { from: string; to: string; kind: string }[];
  notes?: { text: string; anchor: Anchor }[];
  ramp?: { label: string; bins: string[] };
  line_kinds?: { kind: string; label: string; style?: "solid" | "dashed" | "dotted"; arrow?: boolean }[];
  tables?: Table[];
};

const DASH = { solid: undefined, dashed: "6 4", dotted: "2 4" } as const;
const REGION = ["--map-region-a", "--map-region-b", "--map-region-c", "--map-region-d", "--map-region-e"];

function Key({ map }: { map: MapData }) {
  const kinds = Array.from(new Set((map.lines ?? []).map((line) => line.kind))).sort();
  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-muted">
      {map.ramp && (
        <span className="inline-flex items-center gap-2">
          <span>{map.ramp.label}</span>
          <span className="inline-grid gap-0.5 text-center" style={{ gridTemplateColumns: `repeat(${map.ramp.bins.length}, auto)` }}>
            {map.ramp.bins.map((bin, i) => (
              <span
                key={`s${bin}`}
                className="block h-3 w-10 rounded-sm"
                style={{ background: `var(--map-cx-${shade(i, map.ramp!.bins.length)})`, boxShadow: "inset 0 0 0 1px var(--map-edge)" }}
              />
            ))}
            {map.ramp.bins.map((bin) => (
              <span key={`l${bin}`}>{bin}</span>
            ))}
          </span>
        </span>
      )}
      {kinds.map((kind) => {
        const entry = map.line_kinds?.find((candidate) => candidate.kind === kind);
        return (
          <span key={kind} className="inline-flex items-center gap-2">
            <svg width="34" height="10" aria-hidden="true">
              <path d="M2,5 L32,5" stroke="var(--ink-muted)" strokeWidth="1.6" strokeDasharray={DASH[entry?.style ?? "solid"]} strokeLinecap="round" fill="none" />
              {(entry?.arrow ?? true) && <path d="M26,2 L32,5 L26,8" stroke="var(--ink-muted)" strokeWidth="1.6" fill="none" />}
            </svg>
            {entry?.label ?? kind}
          </span>
        );
      })}
    </div>
  );
}

function TableView({ table, hot, setHot }: { table: Table; hot: string | null; setHot: (id: string | null) => void }) {
  return (
    <section aria-label={table.title} className="mt-6">
      <h3 className="m-0 mb-2 text-base font-semibold">{table.title}</h3>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-[0.8125rem] tabular-nums">
          <thead>
            <tr>
              {table.columns.map((column) => (
                <th key={column.label} className={`border-b border-rule px-2 pt-1 pb-1.5 text-xs font-medium text-muted ${column.numeric ? "text-right" : "text-left"}`}>
                  {column.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {table.rows.map((row, r) => (
              <tr
                key={`${row.item ?? ""}${r}`}
                data-id={row.item}
                className={row.item !== undefined && row.item === hot ? "bg-accent-soft" : ""}
                onPointerEnter={() => setHot(row.item ?? null)}
                onPointerLeave={() => setHot(null)}
              >
                {row.cells.map((cell, c) => (
                  <td key={table.columns[c]?.label ?? c} className={`border-b border-rule px-2 py-1.5 align-top ${table.columns[c]?.numeric ? "text-right" : ""}`}>
                    {cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {table.caption && <p className="m-0 mt-2 text-xs text-muted">{table.caption}</p>}
    </section>
  );
}

/** Draws a `map/layout/v1` record: regions, shaded items, a style for each
 * kind of line, notes placed from their anchors, a key and its tables. Items
 * with no stored position are not drawn. */
export function MapView({ data: map }: { data: MapData }) {
  const [hot, setHot] = useState<string | null>(null);
  const marks = new Map<string, Mark>();
  for (const item of map.items) {
    if (item.at) marks.set(item.id, { x: item.at.x, y: item.at.y, r: item.radius ?? RADIUS });
  }
  const bins = map.ramp?.bins.length ?? 0;
  return (
    <div>
      <div className="overflow-x-auto">
        <svg viewBox={`0 0 ${map.sheet.width} ${map.sheet.height}`} role="img" aria-label={map.title} className="block h-auto w-full" style={{ fontFamily: "var(--font-ui)" }}>
          <defs>
            <marker id="map-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
              <path d="M0,1 L9,5 L0,9 z" fill="var(--ink-muted)" />
            </marker>
          </defs>
          {(map.regions ?? []).map((region, i) => (
            <g key={region.id}>
              <rect x={region.x} y={region.y} width={region.width} height={region.height} rx="14" fill={`var(${REGION[i % REGION.length]})`} />
              <text x={region.x + 12} y={region.y + 18} fontSize="11.5" letterSpacing="0.06em" fontWeight="500" fill="var(--ink-muted)" style={{ textTransform: "uppercase" }}>
                {region.title}
              </text>
            </g>
          ))}
          {(map.lines ?? []).map((line) => {
            const [a, b] = [marks.get(line.from), marks.get(line.to)];
            if (!a || !b) return null;
            const entry = map.line_kinds?.find((candidate) => candidate.kind === line.kind);
            const [x1, y1, x2, y2] = lineEnds(a, b);
            const cold = hot !== null && line.from !== hot && line.to !== hot;
            return (
              <path
                key={`${line.from}>${line.to}`}
                d={`M${x1.toFixed(1)},${y1.toFixed(1)} L${x2.toFixed(1)},${y2.toFixed(1)}`}
                stroke="var(--ink-muted)"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeDasharray={DASH[entry?.style ?? "solid"]}
                strokeOpacity={cold ? 0.15 : 0.85}
                fill="none"
                markerEnd={(entry?.arrow ?? true) ? "url(#map-arrow)" : undefined}
              >
                <title>{`${line.from} ${entry?.label ?? line.kind} ${line.to}`}</title>
              </path>
            );
          })}
          {map.items.map((item) => {
            const mark = marks.get(item.id);
            if (!mark) return null;
            const side = item.side ?? "right";
            const box = labelBox(mark, side, item.label);
            const [x, anchor] = side === "right" ? [box.x0, "start"] : side === "left" ? [box.x1, "end"] : [(box.x0 + box.x1) / 2, "middle"];
            return (
              <g
                key={item.id}
                data-id={item.id}
                tabIndex={0}
                className="outline-none"
                onPointerEnter={() => setHot(item.id)}
                onPointerLeave={() => setHot(null)}
                onFocus={() => setHot(item.id)}
                onBlur={() => setHot(null)}
              >
                <circle
                  cx={mark.x}
                  cy={mark.y}
                  r={mark.r}
                  fill={item.bin === undefined ? "var(--rule)" : `var(--map-cx-${shade(item.bin, bins)})`}
                  stroke={hot === item.id ? "var(--accent)" : "var(--map-edge)"}
                  strokeWidth={hot === item.id ? 3 : 1}
                />
                {item.label.map((text, i) => (
                  <text key={`${i}${text}`} x={x} y={box.y0 + 11 + i * 15} textAnchor={anchor as "start" | "end" | "middle"} fontSize={i === 0 ? 14 : 12.5} fontWeight={i === 0 ? 500 : 400} fill={i === 0 ? "var(--ink)" : "var(--ink-muted)"}>
                    {text}
                  </text>
                ))}
                <title>{item.label.join(", ")}</title>
              </g>
            );
          })}
          {(map.notes ?? []).map((note) => {
            let point: { x: number; y: number; anchor: "start" | "middle" | "end" } | undefined;
            if ("item" in note.anchor) {
              const mark = marks.get(note.anchor.item.id);
              if (mark) point = { x: mark.x + note.anchor.item.dx, y: mark.y + note.anchor.item.dy, anchor: "start" };
            } else {
              const [a, b] = [marks.get(note.anchor.line.from), marks.get(note.anchor.line.to)];
              if (a && b) point = lineNote(a, b, note.anchor.line.along, note.anchor.line.offset ?? OFFSET);
            }
            if (!point) return null;
            return (
              <text key={note.text} x={point.x} y={point.y} textAnchor={point.anchor} fontSize="12.5" fill="var(--ink-muted)">
                {note.text}
              </text>
            );
          })}
        </svg>
      </div>
      <div className="mt-3">
        <Key map={map} />
      </div>
      {(map.tables ?? []).map((table) => (
        <TableView key={table.id} table={table} hot={hot} setHot={setHot} />
      ))}
    </div>
  );
}
