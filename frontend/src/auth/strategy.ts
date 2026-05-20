/**
 * dev-pulse auth strategy.
 *
 * Stage 3 of the frontend job. Phase 4 of dp-server ships two ways to
 * authenticate: GitHub OAuth (operator-facing) and a local
 * email/password user table from `starter-auth-users` (seeded by the
 * Phase 6 CLI's `claim` command). The frontend speaks the second one
 * because the login form belongs to the SPA — OAuth is a top-bar
 * "Continue with GitHub" link wired later.
 *
 * `sessionStrategy` from `@nube/starter-ui-core/auth` is exactly that
 * shape: `POST /auth/login { email, password }` -> sets the `sas_*`
 * session cookie -> the AuthProvider then probes `GET /auth/me` and
 * caches the `MeResponse`.
 */

import { sessionStrategy, type AuthStrategy } from "@nube/starter-ui-core/auth";

/** dp-server's local-password login lives at `POST /auth/login`, which
 *  is exactly what `sessionStrategy` already targets. Re-export so the
 *  rest of the app imports one symbol from a stable file. */
export const authStrategy: AuthStrategy = sessionStrategy;
