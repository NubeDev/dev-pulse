/**
 * 404 page — rendered when the hash route doesn't match any known
 * section. The minimal hash router (see `routes.ts`) defaults
 * unknown sections to "reports" so deep-link typos still land on
 * something useful; this page is reserved for the few routes we
 * explicitly mark unknown (e.g. `#/foo/bar` after the section-name
 * gate in `isKnownRoute` rejects "foo").
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
    <Card data-testid="not-found">
      <CardHeader>
        <CardTitle>Page not found</CardTitle>
        <CardDescription>
          No section matches <code>{route}</code>.
        </CardDescription>
      </CardHeader>
      <CardContent style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
        <Button asChild>
          <a href="#/reports">Go to reports</a>
        </Button>
        <Button variant="outline" asChild>
          <a href="#/directory">Open directory</a>
        </Button>
      </CardContent>
    </Card>
  );
}
