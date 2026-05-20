import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
// `@kit/theme` reaches the kit's pure theme module without pulling
// the package barrel into the type-check graph (which would trip
// the React 18/19 typing mismatch on upstream kit primitives).
import { ThemeProvider } from "@kit/theme";

import "./globals.css";
import { App } from "./app.jsx";

/**
 * One QueryClient per browser session. The report pages added in stage
 * 4+ hang their `useQuery` hooks off this client. Defaults are tuned for
 * an operator UI: no aggressive refetches, but stale-on-focus is on so
 * the "Data as of <ts>" banner updates after the user comes back to the
 * tab. Stage 9 widens retry to 1 attempt so a transient blip on the
 * dev-proxy doesn't permanently red-flag a panel — the user can also
 * hit the in-page Retry buttons / error-boundary reset.
 */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: true,
      retry: 1,
    },
    mutations: {
      retry: 0,
    },
  },
});

const root = document.getElementById("root");
if (!root) throw new Error("missing #root in index.html");

createRoot(root).render(
  <StrictMode>
    <ThemeProvider defaultTheme="system">
      <QueryClientProvider client={queryClient}>
        <App />
      </QueryClientProvider>
    </ThemeProvider>
  </StrictMode>,
);
