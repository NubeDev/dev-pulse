import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// dp-server listens on :3000 in dev (see crates/dp-server). Proxy the
// REST surfaces (auth from starter-auth-users, reports/directory/admin
// from dp-rest, plus /health and /openapi.json) so cookies + same-origin
// work without backend CORS config — matches the notes example pattern.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      // `@nube/starter-ui-kit` ships source-only and self-references via
      // `@/` aliases at build time. Mirror it here so Vite resolves
      // those imports the same way tsc does (see tsconfig#paths).
      "@": path.resolve(
        __dirname,
        "../../starter/packages/starter-ui-kit/src",
      ),
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/auth": "http://localhost:3000",
      "/reports": "http://localhost:3000",
      "/directory": "http://localhost:3000",
      "/admin": "http://localhost:3000",
      "/health": "http://localhost:3000",
      "/openapi.json": "http://localhost:3000",
    },
  },
});
