import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// When VITE_USE_MOCK_REPORTS=1 we also stub the auth endpoints so the
// SPA runs end-to-end without a Rust backend (mirrors what the Playwright
// helpers do via page.route()). Lets you demo the UI by just running
// `pnpm dev` — no Postgres, no GitHub App, no sidecar SQLite.
function mockAuthPlugin(): Plugin {
  let authed = false;
  return {
    name: "dev-pulse-mock-auth",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (!req.url) return next();
        if (req.url.startsWith("/auth/me")) {
          if (authed) {
            res.statusCode = 200;
            res.setHeader("content-type", "application/json");
            res.end(
              JSON.stringify({
                subject: "operator-1",
                email: "operator@example.com",
                role: "admin",
              }),
            );
          } else {
            res.statusCode = 401;
            res.setHeader("content-type", "application/json");
            res.end(
              JSON.stringify({ type: "unauthorized", detail: "no session" }),
            );
          }
          return;
        }
        if (req.url.startsWith("/auth/login")) {
          authed = true;
          res.statusCode = 200;
          res.setHeader("content-type", "application/json");
          res.setHeader(
            "set-cookie",
            "sas_session=stub; Path=/; HttpOnly; SameSite=Lax",
          );
          res.end(JSON.stringify({ csrf_token: "stub-csrf-token" }));
          return;
        }
        if (req.url.startsWith("/auth/logout")) {
          authed = false;
          res.statusCode = 200;
          res.setHeader("content-type", "application/json");
          res.end("{}");
          return;
        }
        return next();
      });
    },
  };
}

// dp-server listens on :3000 in dev (see crates/dp-server). Proxy the
// REST surfaces (auth from starter-auth-users, reports/directory/admin
// from dp-rest, plus /health and /openapi.json) so cookies + same-origin
// work without backend CORS config — matches the notes example pattern.
const useMocks = process.env.VITE_USE_MOCK_REPORTS === "1";

export default defineConfig({
  plugins: [react(), tailwindcss(), ...(useMocks ? [mockAuthPlugin()] : [])],
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
    // In mock mode the middleware above handles /auth/*; only proxy
    // when talking to a real Rust backend.
    proxy: useMocks
      ? {}
      : {
          "/auth": "http://localhost:3000",
          "/reports": "http://localhost:3000",
          "/directory": "http://localhost:3000",
          "/admin": "http://localhost:3000",
          "/health": "http://localhost:3000",
          "/openapi.json": "http://localhost:3000",
        },
  },
});
