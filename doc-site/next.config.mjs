import nextra from 'nextra'

const withNextra = nextra({
  // Nextra 4 reads MDX/MD from ./content and builds the page map automatically.
  defaultShowCopyCode: true,
})

// When the site is served from a sub-path (e.g. a GitHub Pages project site),
// set DOCS_BASE_PATH in CI (e.g. "/dev-pulse"). Locally it stays empty so
// `pnpm dev` serves from "/".
const basePath = process.env.DOCS_BASE_PATH || ''

// `output: 'export'` and `next dev` don't mix when a route uses
// `generateStaticParams` (our catch-all [[...mdxPath]]): dev requests for
// webpack HMR assets get routed through the catch-all and crash with
// "missing param … which is required with output: export". Only enable the
// static export for production builds; `pnpm dev` runs as a normal dev server.
const isDev = process.env.NODE_ENV === 'development'

export default withNextra({
  // Static HTML export — no Node server at runtime, suitable for GitHub Pages.
  // Disabled in dev so HMR works (see note above).
  ...(isDev ? {} : { output: 'export' }),
  images: { unoptimized: true },
  basePath,
  // Pages hosts trailing-slash directories more reliably for static export.
  trailingSlash: true,
})
