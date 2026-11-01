/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const hub = process.env.DOMOVOI_HUB_URL ?? "http://127.0.0.1:7420";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": { target: hub, changeOrigin: false },
      "/hooks": { target: hub, changeOrigin: false },
      "/healthz": { target: hub, changeOrigin: false },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    assetsDir: "assets",
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: true,
  },
});
