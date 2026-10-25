import path from "node:path";
import { storybookTest } from "@storybook/addon-vitest/vitest-plugin";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig, mergeConfig } from "vitest/config";
import vite from "./vite.config";

type Names = { root: string; testFileName: string; testName: string; ext: string };

const theme = (name: "light" | "dark") => ({
  extends: true,
  plugins: [storybookTest({ configDir: path.join(import.meta.dirname, ".storybook") })],
  test: {
    name,
    setupFiles: [".storybook/vitest.setup.ts"],
    browser: {
      enabled: true,
      headless: true,
      provider: playwright({ contextOptions: { colorScheme: name } }),
      instances: [{ browser: "chromium" as const }],
      // A screenshot leaves blank what falls below the viewport, so a story
      // must fit within 900 pixels.
      viewport: { width: 1200, height: 900 },
      trace: "retain-on-failure" as const,
      expect: {
        toMatchScreenshot: {
          resolveScreenshotPath: ({ root, testFileName, testName, ext }: Names) =>
            `${root}/goldens/${testFileName.replace(/\.stories\.tsx$/, "")}/${testName}-${name}${ext}`,
        },
      },
    },
  },
});

// Unit tests of the app's own code run in the same browser, without stories.
const unit = {
  extends: true,
  test: {
    name: "unit",
    include: ["src/**/*.test.ts"],
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser: "chromium" as const }],
    },
  },
};

// Each project extends the app's own Vite config, so stories get its plugins.
export default mergeConfig(vite, defineConfig({ test: { projects: [theme("light"), theme("dark"), unit] } }));
