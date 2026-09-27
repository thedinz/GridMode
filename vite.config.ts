import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  root: ".",
  base: "./",
  plugins: [react()],
  build: {
    outDir: "dist/renderer",
    emptyOutDir: true
  },
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // The Rust build writes (and on Windows locks) files under src-tauri/target;
      // watching them crashes the dev server mid-build.
      ignored: ["**/src-tauri/**"]
    }
  }
});
