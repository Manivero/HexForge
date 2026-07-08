import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Конфигурация следует официальным рекомендациям Tauri v2 для Vite:
// фиксированный порт (совпадает с devUrl в tauri.conf.json), игнорирование
// src-tauri в watch-режиме, HMR через фиксированный порт для стабильности
// внутри WebView (не в браузере).
export default defineConfig(async () => ({
  plugins: [react()],

  resolve: {
    // Зеркалирует "paths": { "@/*": ["src/*"] } из tsconfig.json — tsc
    // проверяет типы по этому алиасу, а Vite/Rollup обязаны резолвить его
    // так же на этапе бандлинга, иначе типизация и рантайм расходятся.
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**"],
    },
  },

  envPrefix: ["VITE_", "TAURI_"],

  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));
