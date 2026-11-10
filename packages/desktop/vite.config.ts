import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 在开发时会访问固定端口，端口被占用时应直接失败而不是自动换端口
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "safari15",
    sourcemap: false,
  },
});
