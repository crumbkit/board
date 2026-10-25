import type { Meta, StoryObj } from "@storybook/react-vite";
import { nested, task } from "../fixtures/record";
import { RecordView } from "./Record";

const meta = { component: RecordView } satisfies Meta<typeof RecordView>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Flat: Story = { args: { data: task } };
export const Nested: Story = { args: { data: nested } };
