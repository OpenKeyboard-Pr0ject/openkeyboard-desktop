import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const webAppRoot = fileURLToPath(new URL(process.env.OPENKEYBOARD_WEB_ROOT ?? "../openkeyboard/", import.meta.url));
const webAppSource = `${webAppRoot}src`;
const webAppTests = `${webAppRoot}tests`;
const tauriDevHost = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@openkeyboard": webAppSource, "@openkeyboard-tests": webAppTests },
    dedupe: ["react", "react-dom"],
  },
  server: {
    port: 1420,
    strictPort: true,
    host: tauriDevHost || false,
    fs: { allow: [fileURLToPath(new URL(".", import.meta.url)), webAppSource] },
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    outDir: "dist",
  },
});
