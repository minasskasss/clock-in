/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/ and https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],

  // Keep Rust errors visible in the terminal.
  clearScreen: false,
  server: {
    // Tauri expects a fixed port; fail if it is taken.
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // Tauri uses Chromium (WebView2) on Windows and the system WebView on Android.
    // Chrome 91 is the oldest Android System WebView the app supports; the
    // Kotlin plugin warns below it (`Permissions.MIN_WEBVIEW`, DECISIONS).
    // The emulator-only test APK (`tools/build-apk.ps1 -Emulator`) lowers it to
    // the Android 11 emulator image's built-in WebView; never phone or prod builds.
    target: ["es2022", process.env.CLOCKIN_WEB_TARGET ?? "chrome91"],
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: true,
  },
});
