import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";
import { change, files } from "../fixtures/diff";
import { Diff } from "./Diff";

const meta = { component: Diff } satisfies Meta<typeof Diff>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Change: Story = {
  args: { data: change, slot: "change" },
  // Each line number links to its line, keeping the route before the anchor:
  // `#<path>#diff-<slot>-<file>-R<new>`, or `-L<old>` for a removed line.
  play: async ({ canvasElement }) => {
    const removed = canvasElement.querySelector("#diff-change-0-L11");
    await expect(removed).not.toBeNull();
    const links = [...canvasElement.querySelectorAll<HTMLAnchorElement>("td a")];
    await expect(links.length).toBeGreaterThan(0);
    for (const link of links) await expect(link.getAttribute("href")).toMatch(/^#[^#]*#diff-change-0-[LR]\d+$/);
  },
};

export const SeveralFiles: Story = {
  args: { data: files, slot: "change" },
  // Board escapes hidden characters; the app marks each line that held one.
  play: async ({ canvasElement }) => {
    await expect(canvasElement.textContent).toContain("src/admin\\u{202e}txt.rs");
    const marked = canvasElement.querySelectorAll('[role="note"]');
    await expect(marked.length).toBe(1);
    await expect(marked[0].closest("tr")!.textContent).toContain("\\u{202e}");
  },
};
