/**
 * Theme toggle — opens a shadcn DropdownMenu with Light / Dark /
 * System options. The selected variant is persisted by
 * `<ThemeProvider>` (see `@nube/starter-ui-kit/theme`).
 *
 * Replaces the prior cycle-on-click button: explicit options are
 * easier to discover and match the shadcn "mode toggle" pattern
 * shipped with the upstream starter. The icon-only trigger keeps
 * the header compact at mobile widths; `aria-label` carries the
 * semantic state.
 */

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@nube/starter-ui-kit/components/dropdown-menu";
import { Button } from "@nube/starter-ui-kit/components/button";
// See note in `main.tsx` — the theme module isn't in the package's
// `exports` map, so we resolve it via the `@/` alias instead of the
// `@nube/starter-ui-kit/theme` subpath (which doesn't exist).
import { useTheme } from "@/theme";
import type { Theme } from "@/theme/types";

const LABEL: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  system: "System",
};
const GLYPH: Record<Theme, string> = {
  // Plain glyphs so we don't pull in an icon library — the UI kit
  // ships @hugeicons but those add bytes we don't need for a header
  // button. The glyph rotates with the active theme so the trigger
  // mirrors the current selection.
  light: "☀",
  dark: "☾",
  system: "◐",
};
const OPTIONS: ReadonlyArray<Theme> = ["light", "dark", "system"];

export function ThemeToggle(): JSX.Element {
  const { theme, setTheme } = useTheme();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          aria-label={`Theme: ${LABEL[theme]}. Click to change.`}
          title={LABEL[theme]}
          data-testid="theme-toggle"
          data-theme={theme}
          className="min-w-9"
        >
          <span aria-hidden className="text-base leading-none">
            {GLYPH[theme]}
          </span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-32">
        {OPTIONS.map((opt) => (
          <DropdownMenuItem
            key={opt}
            onSelect={() => setTheme(opt)}
            data-testid={`theme-toggle-${opt}`}
            data-active={theme === opt}
            aria-checked={theme === opt}
          >
            <span aria-hidden className="mr-2 text-base leading-none">
              {GLYPH[opt]}
            </span>
            {LABEL[opt]}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
