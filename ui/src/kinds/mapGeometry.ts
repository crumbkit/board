/** Where a reader sees each part of a map. These are the constants and rules
 * of board-plugin-map's geom.rs, so the app places text as `board map check`
 * estimated it would be placed. */

export type Side = "right" | "left" | "below" | "above";
export type Mark = { x: number; y: number; r: number };
export type Box = { x0: number; y0: number; x1: number; y1: number };

const NAME_WIDTH = 7.4;
const DETAIL_WIDTH = 6.5;
export const RADIUS = 8;
export const OFFSET = 9;
export const LINE_HEIGHT = 15;

/** The box a label of `lines` takes on `side` of `mark`. */
export function labelBox(mark: Mark, side: Side, lines: string[]): Box {
  const r = mark.r + 1;
  const width = Math.max(...lines.map((line, i) => [...line].length * (i === 0 ? NAME_WIDTH : DETAIL_WIDTH)));
  const more = lines.length - 1;
  const height = lines.length * LINE_HEIGHT;
  let x0: number;
  let y: number;
  switch (side) {
    case "right":
      [x0, y] = [mark.x + r + 6, mark.y + 5 - more * 7.5];
      break;
    case "left":
      [x0, y] = [mark.x - r - 6 - width, mark.y + 5 - more * 7.5];
      break;
    case "below":
      [x0, y] = [mark.x - width / 2, mark.y + r + 15];
      break;
    case "above":
      [x0, y] = [mark.x - width / 2, mark.y - r - 10 - more * LINE_HEIGHT];
      break;
  }
  return { x0, y0: y - 11, x1: x0 + width, y1: y - 11 + height };
}

/** The segment a line from `a` to `b` is drawn as, with a gap at each end. */
export function lineEnds(a: Mark, b: Mark): [number, number, number, number] {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = Math.hypot(dx, dy) || 1;
  const [ux, uy] = [dx / length, dy / length];
  return [a.x + ux * (a.r + 2), a.y + uy * (a.r + 2), b.x - ux * (b.r + 3), b.y - uy * (b.r + 3)];
}

/** The point of a note on the line from `a` to `b`, `along` of the way and `offset` across it. */
export function lineNote(a: Mark, b: Mark, along: number, offset: number): { x: number; y: number; anchor: "start" | "middle" | "end" } {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = Math.hypot(dx, dy) || 1;
  let [nx, ny] = [-dy / length, dx / length];
  if (nx < 0) [nx, ny] = [-nx, -ny];
  const across = nx * offset;
  const anchor = across > 2 ? "start" : across < -2 ? "end" : "middle";
  return {
    x: Math.floor(a.x + dx * along + nx * offset + 0.5),
    y: Math.floor(a.y + dy * along + ny * offset + 4 + 0.5),
    anchor,
  };
}

/** The shade, 1 to 5, of bin `bin` of `bins`, spread over the five the app holds. */
export function shade(bin: number, bins: number): number {
  if (bins < 2) return 1;
  return 1 + Math.ceil(Math.floor((bin * 8) / (bins - 1)) / 2);
}
