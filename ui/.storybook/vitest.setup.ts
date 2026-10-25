import { setProjectAnnotations } from "@storybook/react-vite";
import { expect } from "vitest";
import { page } from "vitest/browser";
import * as preview from "./preview";

setProjectAnnotations([
  preview,
  {
    async afterEach({ canvasElement }) {
      await expect.element(page.elementLocator(canvasElement)).toMatchScreenshot();
    },
  },
]);
