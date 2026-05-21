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

// dp-server listens on :8731 in dev (Makefile BACK_PORT). Proxy the
// REST surfaces (auth from starter-auth-users, reports/directory/admin
// from dp-rest, plus /health and /openapi.json) so cookies + same-origin
// work without backend CORS config — matches the notes example pattern.
const useMocks = process.env.VITE_USE_MOCK_REPORTS === "1";

export default defineConfig({
  plugins: [react(), tailwindcss(), ...(useMocks ? [mockAuthPlugin()] : [])],
  resolve: {
    alias: {
      // Local `@/*` alias (matches tsconfig.json#paths) so the shadcn
      // dashboard-01 block components — written into `src/components`,
      // `src/lib`, `src/hooks` — resolve correctly in dev and build.
      "@": path.resolve(__dirname, "src"),
      // `@kit/*` reaches into `@nube/starter-ui-kit` source for the
      // theme module (pure React, no UI deps) without importing the
      // package barrel — which would otherwise drag every kit
      // primitive into the type-check graph and trip the React 18/19
      // typing mismatch on upstream files.
      "@kit": path.resolve(
        __dirname,
        "../../starter/packages/starter-ui-kit/src",
      ),
    },
  },
  server: {
    port: 8732,
    strictPort: true,
    // In mock mode the middleware above handles /auth/*; only proxy
    // when talking to a real Rust backend.
    proxy: useMocks
      ? {}
      : {
          "/auth": "http://localhost:8731",
          "/reports": "http://localhost:8731",
          "/orgs": "http://localhost:8731",
          "/users": "http://localhost:8731",
          "/teams": "http://localhost:8731",
          "/home-org": "http://localhost:8731",
          "/admin": "http://localhost:8731",
          "/issues": "http://localhost:8731",
          "/repos": "http://localhost:8731",
          "/projects": "http://localhost:8731",
          "/me": "http://localhost:8731",
          "/pins": "http://localhost:8731",
          "/tags": "http://localhost:8731",
          "/health": "http://localhost:8731",
          "/openapi.json": "http://localhost:8731",
        },
  },
});
