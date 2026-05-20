// Static / repo-wide invariants that round out the stage-11 smoke
// suite. These don't need a browser, but they live alongside the
// Playwright walkthrough so a single `pnpm test:e2e` run gates ship.
//
//   - no leaderboard affordance anywhere in the rendered SPA source
//     (SCOPE §4 design constraint — no single-score, no ranking UI).
//   - Rust workspace boundary check still green (scripts/check-boundaries.sh).

import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

import { expect, test } from "@playwright/test";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const FRONTEND_ROOT = resolve(__dirname, "..", "..");
const REPO_ROOT = resolve(FRONTEND_ROOT, "..");

/** Walk `dir` recursively yielding every regular file under it. */
function* walk(dir: string): Generator<string> {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist" || entry.name === "tests") continue;
      yield* walk(full);
    } else if (entry.isFile()) {
      yield full;
    }
  }
}

test.describe("static invariants", () => {
  test("no leaderboard / single-score affordance in frontend source", () => {
    const src = resolve(FRONTEND_ROOT, "src");
    // The forbidden tokens — kept narrow so harmless mentions in
    // doc-comments that explicitly disavow the affordance still pass.
    // We test for the **rendered** strings users would see, not for
    // the word "leaderboard" in a comment.
    const forbidden: ReadonlyArray<RegExp> = [
      // JSX text nodes / button labels.
      />\s*Leaderboard\s*</i,
      />\s*Top performers\s*</i,
      />\s*Ranking\s*</i,
      />\s*Score\s*</,
      // aria / title / placeholder attributes.
      /aria-label=["'][^"']*leaderboard/i,
      /title=["'][^"']*leaderboard/i,
      /placeholder=["'][^"']*leaderboard/i,
    ];

    const offenders: string[] = [];
    for (const file of walk(src)) {
      if (!/\.(t|j)sx?$/.test(file)) continue;
      const body = readFileSync(file, "utf8");
      for (const re of forbidden) {
        const m = re.exec(body);
        if (m) offenders.push(`${file}: ${m[0]}`);
      }
    }
    expect(offenders, `forbidden leaderboard affordance(s):\n${offenders.join("\n")}`).toEqual([]);
  });

  test("Rust boundary check (scripts/check-boundaries.sh) still green", () => {
    // The check exits non-zero on violation; execFileSync would throw,
    // which Playwright reports as a failure. We capture stdout so the
    // "OK" line ends up in the trace on success.
    const out = execFileSync("bash", [resolve(REPO_ROOT, "scripts", "check-boundaries.sh")], {
      cwd: REPO_ROOT,
      encoding: "utf8",
    });
    expect(out).toContain("check-boundaries: OK");
  });

  test("pnpm build produces dist/ under 2MB gzipped", () => {
    // Build is idempotent and fast (~1s); we re-run it here so the
    // size check reflects the current tree, not whatever was last
    // built. Output is silenced unless the build fails.
    execFileSync("pnpm", ["build"], {
      cwd: FRONTEND_ROOT,
      stdio: "pipe",
      encoding: "utf8",
    });

    const dist = resolve(FRONTEND_ROOT, "dist");
    let totalGzip = 0;
    for (const file of walk(dist)) {
      const buf = readFileSync(file);
      // Some assets (fonts, images) are already compressed; gzipping
      // them again still gives a meaningful upper-bound for "what the
      // CDN serves." For our current build the dist is pure JS+CSS+HTML.
      totalGzip += gzipSync(buf).length;
      // Tickle statSync just to confirm the file is real (cheap sanity).
      statSync(file);
    }

    const TWO_MB = 2 * 1024 * 1024;
    expect(
      totalGzip,
      `dist/ gzipped is ${(totalGzip / 1024).toFixed(1)} KiB (budget: 2048 KiB)`,
    ).toBeLessThan(TWO_MB);
  });
});
