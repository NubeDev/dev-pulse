/**
 * 404 page — rendered when the hash route doesn't match any known
 * section. The minimal hash router (see `routes.ts`) defaults
 * unknown sections to "reports" so deep-link typos still land on
 * something useful; this page is reserved for the few routes we
 * explicitly mark unknown (e.g. `#/foo/bar` after the section-name
 * gate in `isKnownRoute` rejects "foo").
 *
 * Layout: a centred `Card` with the missing route shown in a
 * shadcn-style inline `<code>` block so it's instantly recognisable
 * as the offending URL fragment.
 */

import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";

import { useRoute } from "../routes.js";

export function NotFoundPage(): JSX.Element {
  const route = useRoute();
  return (
    <div className="flex w-full items-center justify-center py-12">
      <Card data-testid="not-found" className="w-full max-w-md text-center">
        <CardHeader className="items-center">
          <CardTitle>Page not found</CardTitle>
          <CardDescription>
            No section matches{" "}
            <code className="relative rounded bg-muted px-[0.3rem] py-[0.2rem] font-mono text-sm">
              {route}
            </code>
            .
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap justify-center gap-2">
          <Button asChild>
            <a href="#/reports">Go to reports</a>
          </Button>
          <Button variant="outline" asChild>
            <a href="#/directory">Open directory</a>
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
