import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const webAppRoot = fileURLToPath(new URL(process.env.OPENKEYBOARD_WEB_ROOT ?? "../openkeyboard/", import.meta.url));

export default defineConfig({
  resolve: {
    alias: { "@openkeyboard": `${webAppRoot}src`, "@openkeyboard-tests": `${webAppRoot}tests` },
  },
  test: {
    include: ["tests/**/*.test.ts"],
    environment: "node",
  },
});
