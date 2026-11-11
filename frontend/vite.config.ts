import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The backend requires an access token. In dev the proxy adds it, read from
// RUSTMAN_TOKEN or the token file the desktop backend creates.
function devToken(): string {
  if (process.env.RUSTMAN_TOKEN) return process.env.RUSTMAN_TOKEN.trim();
  try {
    return readFileSync(join(homedir(), ".rustman", "agent-token"), "utf8").trim();
  } catch {
    return "";
  }
}

// In dev, proxy API calls to the rustman-desktop backend.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:51730",
        ws: true,
        headers: devToken() ? { Authorization: `Bearer ${devToken()}` } : {},
      },
    },
  },
});
