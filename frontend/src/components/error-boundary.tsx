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
 */

import { Component, type ErrorInfo, type ReactNode } from "react";
import { Button } from "@nube/starter-ui-kit/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@nube/starter-ui-kit/components/card";

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
      <Card data-testid="error-boundary" role="alert">
        <CardHeader>
          <CardTitle>Something went wrong rendering {scope}.</CardTitle>
          <CardDescription>
            The error has been logged to the browser console. Try again, or
            reload the page if the problem persists.
          </CardDescription>
        </CardHeader>
        <CardContent style={{ display: "grid", gap: "0.75rem" }}>
          <pre
            style={{
              fontSize: "0.8125rem",
              color: "var(--muted-foreground)",
              background: "var(--muted)",
              padding: "0.75rem",
              borderRadius: "var(--radius-sm, 0.375rem)",
              overflow: "auto",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              margin: 0,
            }}
          >
            {this.state.err.message}
          </pre>
          <div style={{ display: "flex", gap: "0.5rem" }}>
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
        </CardContent>
      </Card>
    );
  }
}
