import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In dev, proxy API calls to the rustman-desktop backend.
export default defineConfig({
  plugins: [react()],
  server: { proxy: { "/api": { target: "http://127.0.0.1:51730", ws: true } } },
});
