/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/**
 * The main window's own Content-Security-Policy (ADR-0023 section 4). Tauri
 * applies one CSP to every window, and that one lists Twilio's hosts for the
 * phone window. Browsers enforce every policy that applies, so this stricter
 * one keeps the main window on this computer: no remote host at all. Web mode
 * sends the same rule as a header (os-server PAGE_CSP). Added at build time
 * only, so the dev server's own scripts still load.
 */
export const MAIN_CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; " +
  "font-src 'self'; media-src 'self' asset: http://asset.localhost blob:; " +
  "connect-src 'self' ipc: http://ipc.localhost ws://127.0.0.1:* ws://localhost:*; object-src 'none'; base-uri 'none'";

function mainWindowPolicy(): Plugin {
  return {
    name: "openbark-main-csp",
    apply: "build",
    transformIndexHtml: {
      order: "pre",
      handler(html, ctx) {
        if (!ctx.filename.endsWith("index.html")) return html;
        return html.replace(
          "<meta charset=\"UTF-8\" />",
          `<meta charset="UTF-8" />\n    <meta http-equiv="Content-Security-Policy" content="${MAIN_CSP}" />`,
        );
      },
    },
  };
}

// Tauri expects a fixed dev port and no browser auto-open. Two pages: the app,
// and the phone window (phone.html), the only one that bundles Twilio's SDK.
export default defineConfig({
  plugins: [react(), mainWindowPolicy()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2023",
    rollupOptions: {
      input: {
        main: "index.html",
        phone: "phone.html",
      },
    },
  },
  test: { environment: "jsdom" },
});
