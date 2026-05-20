import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import "./globals.css";
import { App } from "./app.jsx";

/**
 * One QueryClient per browser session. The report pages added in stage
 * 4+ hang their `useQuery` hooks off this client; stage 3 just gets it
 * in place so we don't re-mount providers later. Defaults are tuned for
 * an operator UI: no aggressive refetches, but stale-on-focus is on so
 * the "Data as of <ts>" banner updates after the user comes back to the
 * tab.
 */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: true,
      retry: false,
    },
  },
});

const root = document.getElementById("root");
if (!root) throw new Error("missing #root in index.html");

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
