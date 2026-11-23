import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";
import { queue, services, work } from "../fixtures/map";
import { MapView } from "./Map";

const meta = {
  component: MapView,
  decorators: [
    (Story) => (
      <div style={{ maxWidth: 560 }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof MapView>;
export default meta;
type Story = StoryObj<typeof meta>;

// Each item is drawn once, each line has its style, and the key names the
// ramp and each kind of line.
export const Services: Story = {
  args: { data: services },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll("g[data-id]").length).toBe(services.items.length);
    await expect(canvasElement.querySelectorAll("path[stroke-dasharray='6 4']").length).toBeGreaterThan(0);
    await expect(canvasElement.textContent).toContain("Open tasks");
    await expect(canvasElement.textContent).toContain("run-time link");
  },
};

// A table beside the map, with a row for an item that lights it.
export const WithATable: Story = {
  args: { data: work },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll("tbody tr").length).toBe(4);
    await expect(canvasElement.textContent).toContain("Every area");
  },
};

// The queue in five lanes: each lane is a region, each item sits in its
// lane, and the arrows run from an item to the item it builds after.
export const Queue: Story = {
  args: { data: queue },
  play: async ({ canvasElement }) => {
    await expect(canvasElement.querySelectorAll("g[data-id]").length).toBe(queue.items.length);
    await expect(canvasElement.querySelectorAll("svg[role='img'] rect").length).toBe(queue.regions!.length);
    await expect(canvasElement.textContent).toContain("builds after");
    await expect(canvasElement.textContent).toContain("Waits on");
  },
};
