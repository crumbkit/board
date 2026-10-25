import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect } from "storybook/test";
import { long, short } from "../fixtures/markdown";
import { Markdown } from "./Markdown";

const meta = { component: Markdown } satisfies Meta<typeof Markdown>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Short: Story = { args: { data: short } };
export const WithContents: Story = {
  args: { data: long },
  // Every link within the page, footnotes included, keeps the route before
  // its anchor: `#<path>#<id>`. Heading ids carry a prefix, so a heading
  // cannot take an id the app uses, such as `root`.
  play: async ({ canvasElement }) => {
    const headings = [...canvasElement.querySelectorAll("h2[id], h3[id]")].filter((h) => h.id !== "footnote-label");
    await expect(headings.length).toBeGreaterThan(0);
    for (const heading of headings) await expect(heading.id).toMatch(/^heading-/);
    const links = [...canvasElement.querySelectorAll<HTMLAnchorElement>('a[href^="#"]')];
    await expect(links.some((link) => link.getAttribute("href")!.includes("fn-1"))).toBe(true);
    for (const link of links) await expect(link.getAttribute("href")).toMatch(/^#[^#]*#./);
  },
};
