/**
 * Theme toggle — cycles light → dark → system, persisted by
 * `<ThemeProvider>` (see `@nube/starter-ui-kit/theme`).
 *
 * Sits in the app shell header next to the Logout button. The
 * icon-only label keeps the header compact at mobile widths;
 * `aria-label` carries the semantic state.
 */

import { Button } from "@nube/starter-ui-kit/components/button";
// See note in `main.tsx` — the theme module isn't in the package's
// `exports` map, so we resolve it via the `@/` alias instead of the
// `@nube/starter-ui-kit/theme` subpath (which doesn't exist).
import { useTheme } from "@/theme";
import type { Theme } from "@/theme/types";

const ORDER: ReadonlyArray<Theme> = ["light", "dark", "system"];
const LABEL: Record<Theme, string> = {
  light: "Light theme",
  dark: "Dark theme",
  system: "System theme",
};
const GLYPH: Record<Theme, string> = {
  // Plain glyphs so we don't pull in an icon library — the UI kit
  // ships @hugeicons but those add bytes we don't need for one button.
  light: "☀",
  dark: "☾",
  system: "◐",
};

export function ThemeToggle(): JSX.Element {
  const { theme, setTheme } = useTheme();
  function cycle(): void {
    const i = ORDER.indexOf(theme);
    const next = ORDER[(i + 1) % ORDER.length] ?? "system";
    setTheme(next);
  }
  return (
    <Button
      variant="outline"
      size="sm"
      onClick={cycle}
      aria-label={`Theme: ${LABEL[theme]}. Click to change.`}
      title={LABEL[theme]}
      data-testid="theme-toggle"
      data-theme={theme}
      style={{ minWidth: "2.25rem" }}
    >
      <span aria-hidden style={{ fontSize: "1rem", lineHeight: 1 }}>
        {GLYPH[theme]}
      </span>
    </Button>
  );
}
