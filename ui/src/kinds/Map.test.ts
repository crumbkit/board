import { createElement } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { MapView, type MapData } from "./Map";

afterEach(() => {
  vi.restoreAllMocks();
});

const at = (x: number, y: number) => ({ x, y, set: "pinned" as const });

// Two notes with one text, two bins with one name and two columns with one
// heading, as the signed services map has two notes "0.3.1".
const repeated: MapData = {
  $type: "map/layout/v1",
  title: "Repeated texts",
  sheet: { width: 400, height: 300 },
  items: [
    { id: "a", label: ["A"], at: at(80, 80), bin: 0 },
    { id: "b", label: ["B"], at: at(280, 200), bin: 1 },
  ],
  lines: [{ from: "a", to: "b", kind: "lib" }],
  notes: [
    { text: "0.3.1", anchor: { item: { id: "a", dx: 12, dy: 24 } } },
    { text: "0.3.1", anchor: { item: { id: "b", dx: 12, dy: 24 } } },
  ],
  ramp: { label: "Open tasks", bins: ["few", "few"] },
  tables: [
    {
      id: "t",
      title: "Both",
      columns: [{ label: "Name" }, { label: "Name" }],
      rows: [{ cells: ["x", "x"] }],
    },
  ],
};

test("a map whose notes, bins and columns repeat a text draws each with a key of its own", async () => {
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  // React reads this flag to know act() wraps the render.
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  await act(async () => root.render(createElement(MapView, { data: repeated })));

  const notes = [...node.querySelectorAll("svg text")].filter((text) => text.textContent === "0.3.1");
  expect(notes).toHaveLength(2);
  const keyed = error.mock.calls.filter((call) => String(call[0]).includes("same key"));
  expect(keyed).toEqual([]);
  await act(async () => root.unmount());
  node.remove();
});
