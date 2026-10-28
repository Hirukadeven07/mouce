/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import { configDefaults } from "vitest/config";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "jsdom",
    setupFiles: "src/test/setup.ts",
    // Agent worktrees under .claude/ hold their own copies of the tests.
    exclude: [...configDefaults.exclude, ".claude/**"],
  },
});
