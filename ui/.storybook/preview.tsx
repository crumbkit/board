import type { Preview } from "@storybook/react-vite";
import "../src/tokens.css";

const preview: Preview = {
  parameters: { layout: "fullscreen" },
  decorators: [
    (Story) => (
      <div className="bg-paper p-6 text-ink">
        <Story />
      </div>
    ),
  ],
};
export default preview;
