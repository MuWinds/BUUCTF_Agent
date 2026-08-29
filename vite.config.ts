/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';

// Tauri 在 dev 时通过固定端口访问前端，端口被占用应当直接失败而非静默换端口
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
  },

  // Tauri 需要固定端口。
  // 注意：不能用 Tauri 默认的 1420 —— Windows 的保留端口段（本机为 1410-1509，
  // 由 Hyper-V/WinNAT 占用）会让绑定直接 EACCES。5173 在保留段之外。
  // host 显式写 127.0.0.1 而非 localhost：后者在 Windows 上优先解析到 ::1。
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || '127.0.0.1',
    hmr: host ? { protocol: 'ws', host, port: 5174 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },

  // Tauri 使用 Chromium(Windows WebView2)，可以放心用现代语法
  build: {
    target: 'chrome105',
    minify: 'esbuild',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },

  // 只测纯逻辑，不渲染组件，所以用 node 环境而非 happy-dom：
  // 少一个依赖，且 setup 里手动接管 requestAnimationFrame 后帧时机完全可控。
  test: {
    environment: 'node',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.ts'],
  },
});
