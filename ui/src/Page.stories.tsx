import type { Meta, StoryObj } from "@storybook/react-vite";
import { short } from "./fixtures/markdown";
import { task } from "./fixtures/record";
import { PageView } from "./Page";

const meta = { component: PageView } satisfies Meta<typeof PageView>;
export default meta;
type Story = StoryObj<typeof meta>;

export const WithAFailedSlot: Story = {
  args: {
    page: {
      binding: "/bindings/review",
      view: "/views/review",
      slots: [
        { name: "summary", kind: "markdown", ref: "/summaries/sync", data: short },
        { name: "task", kind: "record", ref: "/tasks/retry", data: task },
        {
          name: "owner",
          kind: "record",
          ref: "/people/owner",
          failed: {
            code: "board::ref::dangling",
            message: "/people/owner, at /slots/owner/ref, does not resolve",
            hint: "run `board ls /people` to see what is there",
          },
        },
      ],
    },
  },
};
