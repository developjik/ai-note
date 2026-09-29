import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  test: { environment: "jsdom" },
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "es2022",
  },
});
