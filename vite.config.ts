import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Tauri owns the terminal output.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    watch: {
      // Rust changes are rebuilt by Tauri, not Vite.
      ignored: ["**/src-tauri/**"],
    },
    ...(host
      ? {
          hmr: {
            protocol: "ws" as const,
            host,
            port: 1421,
          },
        }
      : {}),
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Tauri uses the OS webview: modern on Linux/Windows, WebKit on macOS.
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },
});
