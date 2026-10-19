import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  test: { environment: "jsdom", globals: true },
});
