/**
 * Minimal dev-pulse shell. Later stages add the AuthProvider from
 * `@nube/starter-ui-core/auth`, the QueryClientProvider, and the
 * SCOPE §11.5 report pages (headline + table + trend, three-lens
 * toggle). This stage 1 just renders enough markup to prove the
 * Vite + Tailwind + starter-ui-kit token pipeline boots.
 */
export function App(): JSX.Element {
  return (
    <main style={{ padding: "2rem", maxWidth: "48rem", margin: "0 auto" }}>
      <h1 style={{ fontSize: "1.5rem", fontWeight: 600, marginBottom: "0.5rem" }}>
        dev-pulse
      </h1>
      <p style={{ color: "var(--muted-foreground)" }}>
        Frontend scaffold ready. Stages 2+ wire auth, react-query, and the
        report pages.
      </p>
    </main>
  );
}
