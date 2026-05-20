/**
 * App-level error boundary — stage 9 polish.
 *
 * Wraps each section pane so a render-time exception in (say) the
 * freshness page doesn't blank the whole shell. Recovers via a
 * "Retry" button that resets the boundary; the offending child
 * gets re-mounted, react-query refetches stale queries, and the
 * operator can keep working without a full-page reload.
 *
 * Network / query errors are surfaced inline by the pages
 * themselves (`useQuery().error`) — this boundary is for the
 * synchronous "something threw during render" case.
 *
 * Renders the shadcn `Alert` primitive (destructive variant) so
 * the failure mode is visually distinct from a normal Card panel,
 * with a retry `Button` + reload-page secondary action below.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@nube/starter-ui-kit/components/alert";
import { Button } from "@nube/starter-ui-kit/components/button";

export interface ErrorBoundaryProps {
  /** Optional label shown above the error — defaults to "this view". */
  scope?: string;
  /** Reset key — when this value changes the boundary auto-resets,
   *  so navigating between routes clears a previous crash without
   *  the operator clicking "Retry" again. */
  resetKey?: string;
  children: ReactNode;
}

interface State {
  err: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, State> {
  override state: State = { err: null };

  static getDerivedStateFromError(err: Error): State {
    return { err };
  }

  override componentDidCatch(err: Error, info: ErrorInfo): void {
    // eslint-disable-next-line no-console
    console.error("[error-boundary]", err, info.componentStack);
  }

  override componentDidUpdate(prev: ErrorBoundaryProps): void {
    if (this.state.err && prev.resetKey !== this.props.resetKey) {
      this.setState({ err: null });
    }
  }

  private reset = (): void => {
    this.setState({ err: null });
  };

  override render(): ReactNode {
    if (!this.state.err) return this.props.children;
    const scope = this.props.scope ?? "this view";
    return (
      <Alert variant="destructive" data-testid="error-boundary">
        <AlertTitle>Something went wrong rendering {scope}.</AlertTitle>
        <AlertDescription>
          <p>
            The error has been logged to the browser console. Try again, or
            reload the page if the problem persists.
          </p>
          <pre className="m-0 max-w-full overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted px-3 py-2 text-[0.8125rem] text-muted-foreground">
            {this.state.err.message}
          </pre>
          <div className="flex flex-wrap gap-2">
            <Button onClick={this.reset} data-testid="error-boundary-retry">
              Retry
            </Button>
            <Button
              variant="outline"
              onClick={() => window.location.reload()}
            >
              Reload page
            </Button>
          </div>
        </AlertDescription>
      </Alert>
    );
  }
}
