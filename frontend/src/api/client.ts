/**
 * Barrel re-export — preserves the original import path for all consumers.
 */
import { StarterClient, StarterError } from "@nube/starter-client-ts";
import { DevPulseApi } from "./dev-pulse-api.js";

export * from "./schemas/index.js";
export * from "./error.js";
export { DevPulseApi } from "./dev-pulse-api.js";
export { StarterClient, StarterError };

// ---------------------------------------------------------------------------
// Singleton
// ---------------------------------------------------------------------------

const baseUrl = (import.meta.env.VITE_API_BASE_URL ?? "").replace(/\/$/, "");

/** Shared singleton — used by react-query hooks and the auth provider.
 *
 *  `apiPrefix: ""` because dp-server mounts the starter REST surfaces at
 *  the root (`/auth/login`, `/auth/me`, `/reports/...`) rather than under
 *  the `/api/v1` prefix the starter ships by default. See the Vite proxy
 *  config in `vite.config.ts` — every routed path is a sibling of `/`. */
export const api: DevPulseApi = new DevPulseApi(
  new StarterClient({ baseUrl, apiPrefix: "" }),
);
