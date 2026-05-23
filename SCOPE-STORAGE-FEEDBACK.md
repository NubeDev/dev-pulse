# Feedback to `starter` blob-storage scope — from dev-pulse

> **Status (2026-05-23): absorbed into starter.**
> Gaps 1, 2, 3, and 5 landed as first-class items in
> [starter's storage scope](../starter/DOCS/storage/SCOPE.md) — new
> `starter-blob-axum` crate, locked `useBlobUpload` surface,
> isolation guidance table, and reserved `BlobMeta` keys
> (`filename`, `uploaded_by`, `uploaded_at`). Gap 4 (quotas) has
> now also shipped on the SPI + memory + fs + `Namespaced::Quota`
> path (`BlobUsage`, `approximate_usage`,
> `Namespaced::with_quota(...)`). Remaining: `approximate_usage`
> on `starter-blob-s3` / `starter-blob-garage` (list/inventory-
> based, may lag).
> Gap 1's authz closure was expanded to receive a `BlobContext` so
> consumer authz never re-parses keys.
>
> This document is retained as the historical input that drove
> those changes; the canonical specification now lives in starter.

> Companion note to [/home/user/code/rust/starter/DOCS/storage/SCOPE.md](../starter/DOCS/storage/SCOPE.md).
> Written from the perspective of the first real consumer (dev-pulse)
> attempting to wire blob storage for two concrete features:
>
> 1. **Project files / images / general doc store** — a project can have
>    arbitrary files attached (PDFs, screenshots, exports).
> 2. **Project scope pages** — markdown pages per project, with embedded
>    images uploaded inline via the existing
>    [`@uiw/react-md-editor`](frontend/src/components/markdown.tsx)
>    instance already used by the issues surface.
>
> Most of the storage scope is sufficient as written. This document
> lists the **gaps** dev-pulse hit and proposes scope additions that
> would let any consumer (not just dev-pulse) integrate without
> re-rolling the same plumbing each time.

---

## What the existing scope gets right

These are already covered and dev-pulse will use them as-is:

- `BlobStore` trait with `put_bytes` / `put_stream` / `get` / `head` /
  `delete` / `list` / `presign` — exact surface dev-pulse needs.
- `BlobRef` as the persisted handle — dev-pulse will store it as a
  JSON column on `dp_project_files` and `dp_project_scope_pages`.
- `Namespaced` combinator — gives per-project key isolation
  (`Namespaced("project-<id>", engine)`) without a domain change.
- Fs engine for dev / single-node; Garage engine for prod, swappable
  via the §"Swap test".
- `BlobError::{NotFound, Forbidden, PreconditionFailed, Throttled,
  Unsupported, …}` — covers everything dev-pulse needs to map to
  HTTP status codes.

The rest of this document is **additions**, not corrections.

---

## Gap 1 — `BlobProxyRouter` for stable, auth-checked GETs

### Why this matters

The scope ships a feature-gated `axum::Router` only for the *presign*
contract in `starter-blob-fs` and `starter-blob-memory`. That covers
direct PUT-from-browser uploads, which is correct.

For **inline markdown images**, presigned URLs are the wrong primitive:

- A markdown page lives in a database row and is rendered at arbitrary
  times. If its embedded image references are presigned with a TTL,
  every render either has to refresh them server-side (extra latency,
  signing cost) or the markdown body has to be rewritten on every
  edit (lossy round-trip with the editor).
- Per-request auth is the right model: the GET handler decides whether
  *this* viewer is allowed to see *this* `BlobRef` based on the
  enclosing project's ACL.

Today every consumer would write the same ~30-line `axum` handler
that calls `store.head(ref)` for the Content-Type, then streams
`store.get(ref, range)` to the response body, with Range and
If-None-Match handled correctly. That's library-shaped work.

### Proposed scope addition

A new optional item in `starter-spi` (or a tiny `starter-blob-axum`
crate so it doesn't pull `axum` into `spi`):

```
fn blob_proxy_handler<S: BlobStore + 'static>(
    store: Arc<S>,
    authz: impl Fn(&BlobRef, &Request) -> Result<(), BlobError> + Send + Sync + 'static,
) -> axum::Router
```

The handler is responsible for:

- Parsing `BlobRef` from the URL (the same opaque serde form
  `BlobRef` already round-trips through).
- Calling the consumer-supplied `authz` closure — so dev-pulse can
  enforce "viewer must have access to the project this ref belongs
  to" without starter knowing what a project is (B1 stays intact).
- Mapping `BlobError` variants to HTTP status codes (`NotFound`→404,
  `Forbidden`→403, `Throttled`→503 with Retry-After, etc.) — the
  mapping is uniform across consumers and shouldn't be re-derived.
- Forwarding `Range`, `If-None-Match`, and `Accept-Encoding`
  end-to-end where the engine supports it.

### Why this doesn't violate B1

The handler takes consumer authz as a closure; it knows nothing about
domain entities. It is the same shape as the existing presign router
the scope already ships for `fs` and `memory`, just for the GET side.

### Open sub-question

Should `head` and `get` be a single combined call (`get_with_meta`)
on the trait to save a round-trip for small engines like `s3`? Or
is the proxy handler expected to issue both serially and trust
engines to make `head` cheap? Recommendation: leave the trait alone,
document the expectation that `head` is cheap.

---

## Gap 2 — Markdown-editor upload hook (`useBlobUpload`)

### Why this matters

The scope mentions, under §"What does NOT ship here":

> No upload UI. The React side gets a `useBlobUpload` hook in
> `@nube/starter-ui-core` (separate scope doc) that calls `presign`
> and `PUT`s directly to the backend; no widget is shipped in
> `ui-kit`.

That separate scope doc does not yet exist (or is not referenced from
the storage scope). dev-pulse needs this hook *now* for the markdown
editor's paste-image / drop-image flow, and will write a local
version. That version should later be absorbed into starter so the
next consumer doesn't re-roll it.

### Proposed scope addition

A `starter-ui-core` scope doc (or a section in the existing storage
scope) that locks the hook surface:

```ts
function useBlobUpload(opts: {
    presignEndpoint: string;          // POST → { url, headers, ref }
    onUploaded: (ref: BlobRef, meta: BlobMeta) => void;
    maxBytes?: number;
    acceptedTypes?: string[];
}): {
    upload: (file: File) => Promise<BlobRef>;
    progress: number | null;
    error: Error | null;
};
```

Plus a thin **markdown-editor adapter** the hook composes with, so
that any `@uiw/react-md-editor` (or tiptap, or codemirror) instance
gets paste-image / drop-image / toolbar-upload behaviour by passing
one prop:

```ts
const onImageUpload = useBlobUploadForMarkdown({ presignEndpoint, urlFor });
<MDEditor ... onImageUpload={onImageUpload} />
```

The hook is taxonomy-agnostic: it does not know what a "project" is.
The consumer's presign endpoint is what binds an upload to a domain
object.

### Why this matters for B2

If every consumer writes the upload flow themselves, they will be
tempted to round-trip raw keys (URLs) into the markdown body. A
canonical hook that always writes `![](GET-proxy-url-for-{ref})`
keeps the markdown body referring to `BlobRef`s, not engine keys.
The Namespaced/Tiered combinator swap stays non-breaking, which is
the whole point of B2.

---

## Gap 3 — Per-tenant isolation recipe: `Namespaced` vs Garage per-bucket keys

### Why this matters

`starter-blob-garage` lists per-bucket access-key minting as a
feature. `starter-blob-compose` provides `Namespaced`. For an app
like dev-pulse (multi-project, low tenant count, single trust
boundary), `Namespaced("project-<id>", root_store)` is dramatically
simpler than minting a Garage key per project.

The scope does not currently say which path is recommended for which
shape of consumer. Without guidance, a consumer will either:

- Over-engineer by minting Garage keys for every domain object
  (operational nightmare), or
- Under-engineer by skipping namespacing entirely and hoping nobody
  guesses a key (security smell — though B2 already mitigates this
  if the consumer never sees keys).

### Proposed scope addition

A short "choosing isolation" section in the storage scope:

| Need                                      | Use                                  |
| ----------------------------------------- | ------------------------------------ |
| Multi-project, single trust boundary      | `Namespaced("project-<id>", store)`  |
| Multi-tenant, separate trust boundaries   | Garage per-bucket key minting        |
| Multi-tenant, hosted on shared S3         | `Namespaced` + IAM bucket policy     |

dev-pulse will use row 1. Locking this in the scope removes a
recurring "should I…?" from every new consumer.

---

## Gap 4 — Quotas / per-namespace size accounting

### Why this matters

The scope's §"Open questions" already names this:

> **Quotas / accounting.** Out of scope for 0.1; revisit when a real
> consumer needs per-tenant byte caps. The `Namespaced` combinator
> is the natural place to add it.

dev-pulse **is** that real consumer. A project's file dump can grow
unbounded if a user pastes screenshots into scope pages, and we want
to refuse new uploads past, say, 500 MB per project rather than
discovering the limit when Garage fills up.

### Proposed scope addition

Lift this from "open question" to a 0.2 scope item:

- `Namespaced` gains an optional `Quota { max_bytes, max_objects }`.
- The combinator tracks usage in a `BlobStore`-adjacent counter (the
  scope should specify *where* — in the engine's listing, or in a
  side table the consumer owns?).
- On `put_*` exceeding the cap, return
  `BlobError::PayloadTooLarge` (already in the enum — good).

Open sub-question: is the counter authoritative (consistent listing
on every put) or eventually consistent (periodic recount)? For
Garage / S3 the authoritative version is expensive; for `fs` and
`memory` it's trivial. Recommendation: trait method
`fn approximate_usage(prefix) -> BlobUsage` and let engines
implement it however they can.

### Why dev-pulse needs this on a real timeline

Without per-project caps, the only safe MVP is a global cap, which
is the wrong product shape (one noisy project takes the whole org's
budget). With caps, projects become independently bounded — closer
to the "cross-org tenant" model dev-pulse already runs on.

---

## Gap 5 — `BlobMeta` user-metadata for original filename

### Why this matters

The scope says `BlobMeta` has "user-defined string→string metadata
(capped, validated)" but doesn't specify a stable key for the
**original filename**. Every consumer needs to round-trip
`avatar.png` → engine key → back to `avatar.png` when serving as a
download. If each consumer picks their own key (`filename` vs
`original_name` vs `x-filename`), the metadata isn't portable when a
`BlobRef` is moved between consumers.

### Proposed scope addition

Reserve a small set of conventional keys in the `BlobMeta`
user-metadata map, documented in `starter-spi`:

| Key                    | Meaning                                   |
| ---------------------- | ----------------------------------------- |
| `filename`             | Original client-supplied filename, UTF-8  |
| `uploaded_by`          | Opaque consumer-defined principal id      |
| `uploaded_at`          | RFC3339 timestamp                         |

Consumers can add their own keys freely; reserving these three means
every consumer gets the same `Content-Disposition: attachment;
filename="…"` behaviour from `blob_proxy_handler` (Gap 1) for free.

---

## Summary — what dev-pulse will do in the meantime

For each gap above, dev-pulse will ship a **local** implementation
inside this repo, tagged with a `// XXX(starter-blob-feedback)`
comment pointing at this file. Once starter absorbs the gap, the
local version gets deleted.

| Gap | Local landing site in dev-pulse                                                                    |
| --: | -------------------------------------------------------------------------------------------------- |
| 1   | `crates/dp-rest/src/blob_proxy.rs` — axum handler over `dyn BlobStore`                             |
| 2   | `frontend/src/lib/use-blob-upload.ts` + `frontend/src/components/markdown.tsx` upload adapter      |
| 3   | Wiring in `crates/dp-server/src/main.rs` (one line picking `Namespaced` over per-project keys)     |
| 4   | `crates/dp-store-pg/migrations/dp/00NN_project_blob_quota.sql` — usage counter row per project     |
| 5   | Constants module `crates/dp-domain/src/blob_meta_keys.rs` with `FILENAME`, `UPLOADED_BY`, …        |

When the local version is being written, it should be written in the
shape the proposed scope addition specifies, so the eventual absorb
is a move, not a rewrite.

---

## Out of scope for this feedback

The following are non-goals dev-pulse fully agrees with:

- No CAS layer.
- No transcoding / thumbnailing / virus scanning.
- No file-share / public-link product feature.
- No Garage-cluster orchestration.

These should stay non-goals.
