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
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            // The libraries change far less often than the app: their own file stays cached
            // across hub upgrades.
            {
              name: "vendor",
              test: /node_modules[\\/](react|react-dom|react-router|scheduler|@tanstack|openapi-fetch)[\\/]/,
              priority: 20,
            },
            // The Markdown stack is only fetched when Markdown is first shown
            // (src/components/Markdown.tsx); everything it pulls in travels together.
            {
              name: "markdown",
              test: /node_modules[\\/](?!(react|react-dom|react-router|scheduler|@tanstack|openapi-fetch|lucide-react)[\\/])/,
              priority: 10,
            },
          ],
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: true,
  },
});
