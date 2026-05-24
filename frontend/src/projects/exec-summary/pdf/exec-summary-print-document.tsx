/**
 * Printable view of the executive summary.
 *
 * Rendered into a hidden offscreen host by `<PrintableContent>` and
 * captured by `printNode` from `@nube/starter-ui-export`. The browser
 * paginates natively via `@page` rules injected by the library; this
 * document only hints at section breaks with `break-before: page` so
 * each major section opens on a fresh sheet.
 *
 * Skipped sections render the heading with a `(N/A)` suffix and omit
 * the body — keeps the printout from being padded with disclaimers
 * for sections the team explicitly opted out of.
 */

import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  ExecSummaryDto,
  ProjectDto,
  ExecSummarySectionId,
} from "../../../api/client.js";
import { SECTIONS } from "../shared.js";

const INK = "#0f172a";
const MUTED = "#475569";
const FAINT = "#94a3b8";
const RULE = "#e2e8f0";
const PANEL = "#f8fafc";
const ACCENT = "#0f172a";

// Print-local markdown renderer. Uses inline styles so the rendered
// tree is self-contained when the print stylesheet from `printNode`
// isolates the host subtree. Tailwind v4's preflight resets `ul/ol`
// to `list-style: none`, so explicit `listStyleType` is required to
// get bullets back.
const PRINT_MD_COMPONENTS: Components = {
  p: (p) => <p style={{ margin: "3pt 0", lineHeight: 1.45 }} {...p} />,
  ul: (p) => (
    <ul
      style={{
        margin: "3pt 0",
        paddingLeft: "16pt",
        listStyleType: "disc",
        listStylePosition: "outside",
      }}
      {...p}
    />
  ),
  ol: (p) => (
    <ol
      style={{
        margin: "3pt 0",
        paddingLeft: "16pt",
        listStyleType: "decimal",
        listStylePosition: "outside",
      }}
      {...p}
    />
  ),
  li: (p) => (
    <li style={{ margin: "1pt 0", display: "list-item", lineHeight: 1.4 }} {...p} />
  ),
  strong: (p) => <strong style={{ fontWeight: 600, color: INK }} {...p} />,
  em: (p) => <em style={{ fontStyle: "italic" }} {...p} />,
  h1: (p) => (
    <h1 style={{ fontSize: "12pt", fontWeight: 600, margin: "6pt 0 3pt" }} {...p} />
  ),
  h2: (p) => (
    <h2 style={{ fontSize: "11pt", fontWeight: 600, margin: "6pt 0 3pt" }} {...p} />
  ),
  h3: (p) => (
    <h3 style={{ fontSize: "10.5pt", fontWeight: 600, margin: "4pt 0 2pt" }} {...p} />
  ),
  code: (p) => (
    <code
      style={{
        background: PANEL,
        padding: "0 3pt",
        borderRadius: "2pt",
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "0.88em",
      }}
      {...p}
    />
  ),
  pre: (p) => (
    <pre
      style={{
        background: PANEL,
        padding: "6pt 8pt",
        borderRadius: "3pt",
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "9pt",
        whiteSpace: "pre-wrap",
        margin: "4pt 0",
      }}
      {...p}
    />
  ),
  blockquote: (p) => (
    <blockquote
      style={{
        borderLeft: `2pt solid ${RULE}`,
        margin: "4pt 0",
        padding: "0 0 0 8pt",
        color: MUTED,
      }}
      {...p}
    />
  ),
  a: (p) => <a style={{ color: INK, textDecoration: "underline" }} {...p} />,
  table: (p) => (
    <table
      style={{
        borderCollapse: "collapse",
        width: "100%",
        margin: "4pt 0",
        fontSize: "9.5pt",
      }}
      {...p}
    />
  ),
  th: (p) => (
    <th
      style={{
        border: `1px solid ${RULE}`,
        background: PANEL,
        padding: "4pt 6pt",
        textAlign: "left",
        fontWeight: 600,
      }}
      {...p}
    />
  ),
  td: (p) => (
    <td
      style={{
        border: `1px solid ${RULE}`,
        padding: "4pt 6pt",
        verticalAlign: "top",
      }}
      {...p}
    />
  ),
};

function PrintMarkdown({ children }: { children: string }): JSX.Element {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={PRINT_MD_COMPONENTS}>
      {children}
    </ReactMarkdown>
  );
}

export interface ExecSummaryPrintDocumentProps {
  project: ProjectDto;
  data: ExecSummaryDto;
  generatedAt: Date;
}

export function ExecSummaryPrintDocument({
  project,
  data,
  generatedAt,
}: ExecSummaryPrintDocumentProps): JSX.Element {
  const skipped = new Set<string>(data.skipped_sections);
  return (
    <div
      data-testid="exec-summary-print-document"
      style={{
        fontFamily:
          "'Inter Variable', Inter, system-ui, -apple-system, 'Segoe UI', sans-serif",
        fontSize: "10.5pt",
        lineHeight: 1.45,
        color: INK,
      }}
    >
      <CoverPage project={project} data={data} generatedAt={generatedAt} />
      {SECTIONS.map((meta, i) => (
        <SectionBlock
          key={meta.id}
          id={meta.id}
          label={meta.label}
          step={i + 1}
          skipped={skipped.has(meta.id)}
        >
          {renderSectionBody(meta.id, data)}
        </SectionBlock>
      ))}
    </div>
  );
}

function CoverPage({
  project,
  data,
  generatedAt,
}: {
  project: ProjectDto;
  data: ExecSummaryDto;
  generatedAt: Date;
}): JSX.Element {
  const skippedLabels = data.skipped_sections
    .map((id) => SECTIONS.find((s) => s.id === id)?.label ?? id)
    .join(", ");
  const pct = data.completion.percent;
  return (
    <section
      style={{
        breakAfter: "page",
        pageBreakAfter: "always",
        minHeight: "240mm",
        display: "flex",
        flexDirection: "column",
        justifyContent: "space-between",
      }}
    >
      <div>
        <div
          style={{
            fontSize: "9pt",
            textTransform: "uppercase",
            letterSpacing: "0.18em",
            color: MUTED,
            fontWeight: 600,
          }}
        >
          Executive Summary
        </div>
        <h1
          style={{
            fontSize: "32pt",
            fontWeight: 700,
            lineHeight: 1.1,
            margin: "10pt 0 4pt",
            letterSpacing: "-0.01em",
          }}
        >
          {project.name}
        </h1>
        <div style={{ fontSize: "11pt", color: MUTED }}>
          {fmtDate(generatedAt)}
        </div>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: "10pt",
          margin: "20pt 0",
        }}
      >
        <StatCard label="Status" value={statusLabel(data.approval.status)} />
        <StatCard label="Completion" value={`${pct}%`} accent={pct >= 80} />
        <StatCard
          label="Submitted"
          value={
            data.approval.submitted_at
              ? fmtDate(new Date(data.approval.submitted_at))
              : "—"
          }
        />
        <StatCard
          label="Approved"
          value={
            data.approval.approved_at
              ? fmtDate(new Date(data.approval.approved_at))
              : "—"
          }
        />
      </div>

      <ProgressBar percent={pct} />

      <div style={{ marginTop: "16pt" }}>
        <Label>Contents</Label>
        <ol
          style={{
            margin: "6pt 0 0",
            paddingLeft: "0",
            listStyleType: "none",
            columnCount: 2,
            columnGap: "16pt",
            fontSize: "10pt",
          }}
        >
          {SECTIONS.map((s, i) => {
            const isSkipped = data.skipped_sections.includes(s.id);
            return (
              <li
                key={s.id}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  padding: "3pt 0",
                  color: isSkipped ? FAINT : INK,
                  borderBottom: `0.5pt dotted ${RULE}`,
                }}
              >
                <span>
                  <span style={{ color: MUTED, marginRight: "6pt" }}>
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  {s.label}
                  {isSkipped ? " (N/A)" : ""}
                </span>
              </li>
            );
          })}
        </ol>
      </div>

      <div style={{ marginTop: "auto", paddingTop: "16pt" }}>
        {skippedLabels && (
          <div style={{ fontSize: "9.5pt", color: MUTED, marginBottom: "8pt" }}>
            <Label>Marked N/A</Label> {skippedLabels}
          </div>
        )}
        <div
          style={{
            fontSize: "8.5pt",
            color: FAINT,
            borderTop: `1px solid ${RULE}`,
            paddingTop: "6pt",
          }}
        >
          Generated by dev-pulse · executive summary export
        </div>
      </div>
    </section>
  );
}

function StatCard({
  label,
  value,
  accent = false,
}: {
  label: string;
  value: string;
  accent?: boolean;
}): JSX.Element {
  return (
    <div
      style={{
        border: `1px solid ${RULE}`,
        borderLeft: `3pt solid ${accent ? "#059669" : ACCENT}`,
        padding: "8pt 10pt",
        background: PANEL,
      }}
    >
      <div
        style={{
          fontSize: "8pt",
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          color: MUTED,
          fontWeight: 600,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: "14pt",
          fontWeight: 600,
          marginTop: "2pt",
          color: accent ? "#059669" : INK,
        }}
      >
        {value}
      </div>
    </div>
  );
}

function ProgressBar({ percent }: { percent: number }): JSX.Element {
  return (
    <div>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          fontSize: "9pt",
          color: MUTED,
          marginBottom: "3pt",
        }}
      >
        <span>Progress</span>
        <span style={{ fontWeight: 600, color: INK }}>{percent}% complete</span>
      </div>
      <div
        style={{
          height: "4pt",
          background: RULE,
          borderRadius: "2pt",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${percent}%`,
            height: "100%",
            background: percent >= 80 ? "#059669" : ACCENT,
          }}
        />
      </div>
    </div>
  );
}

function SectionBlock({
  id,
  label,
  step,
  skipped,
  children,
}: {
  id: string;
  label: string;
  step: number;
  skipped: boolean;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <section
      data-section-id={id}
      style={{
        breakBefore: "page",
        pageBreakBefore: "always",
        breakInside: "auto",
      }}
    >
      <header
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: "10pt",
          marginBottom: "14pt",
          paddingBottom: "8pt",
          borderBottom: `2pt solid ${ACCENT}`,
        }}
      >
        <div
          style={{
            fontSize: "9pt",
            color: MUTED,
            fontWeight: 600,
            letterSpacing: "0.08em",
          }}
        >
          {String(step).padStart(2, "0")}
        </div>
        <h2
          style={{
            fontSize: "18pt",
            fontWeight: 700,
            margin: 0,
            color: skipped ? FAINT : INK,
            letterSpacing: "-0.005em",
          }}
        >
          {label}
        </h2>
        {skipped && (
          <span
            style={{
              fontSize: "9pt",
              color: FAINT,
              padding: "2pt 6pt",
              border: `1px solid ${RULE}`,
              borderRadius: "2pt",
              textTransform: "uppercase",
              letterSpacing: "0.06em",
              fontWeight: 600,
            }}
          >
            N/A
          </span>
        )}
      </header>
      {!skipped && children}
    </section>
  );
}

function renderSectionBody(
  id: ExecSummarySectionId,
  data: ExecSummaryDto,
): React.ReactNode {
  switch (id) {
    case "summary":
      return <SummaryBody data={data} />;
    case "scope":
      return <ScopeBody data={data} />;
    case "requirements":
      return <RequirementsBody data={data} />;
    case "hardware":
      return <HardwareBody data={data} />;
    case "commercial":
      return <CommercialBody data={data} />;
    case "documents":
      return <DocumentsBody data={data} />;
    case "approval":
      return <ApprovalBody data={data} />;
    case "changelog":
      return <ChangelogBody data={data} />;
  }
}

function SummaryBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const s = data.summary;
  return (
    <>
      <DataPanel
        items={[
          ["Product", s.product_name],
          ["Part Number", s.part_number],
          ["Target Release", s.target_release_date],
        ]}
      />
      <Field label="Objective" value={s.objective} markdown />
      <Field label="Problem" value={s.problem} markdown />
      <Field label="Value" value={s.value} markdown />
      <Field label="Differentiators" value={s.differentiators} markdown />
      <Field label="Success Criteria" value={s.success_criteria} markdown />
    </>
  );
}

function ScopeBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const s = data.scope;
  return (
    <>
      <Field label="In Scope" value={s.in_scope} markdown />
      <Field label="Out of Scope" value={s.out_of_scope} markdown />
      <Field label="Assumptions" value={s.assumptions} markdown />
      <Field label="Dependencies" value={s.dependencies} markdown />
      <Field label="Constraints" value={s.constraints} markdown />
    </>
  );
}

function RequirementsBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const r = data.requirements;
  return (
    <>
      <Field label="Must Have" value={r.must_have} markdown />
      <Field label="Optional" value={r.optional} markdown />
      <Field label="User Interaction" value={r.user_interaction} markdown />
      <Field label="Architecture" value={r.architecture} markdown />
      <Field
        label="Protocols"
        value={r.protocols && r.protocols.length > 0 ? r.protocols.join(", ") : null}
      />
      <DataPanel
        items={[
          ["Power", r.power],
          ["Mounting", r.mounting],
          ["Certification", r.certification],
        ]}
      />
    </>
  );
}

function HardwareBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const h = data.hardware;
  const images = data.images;
  return (
    <>
      <Field label="Features" value={h.hardware_features} markdown />
      <Field label="Physical Notes" value={h.physical_notes} markdown />
      <DataPanel
        items={[
          ["Enclosure", h.enclosure],
          ["Mounting Type", h.mounting_type],
          ["Operating Environment", h.operating_env],
        ]}
      />
      {images.length > 0 && (
        <div style={{ marginTop: "12pt" }}>
          <Label>Reference Images</Label>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(2, 1fr)",
              gap: "10pt",
              marginTop: "6pt",
            }}
          >
            {images.map((img) => (
              <figure key={img.id} style={{ margin: 0, breakInside: "avoid" }}>
                <img
                  src={absoluteUrl(img.url)}
                  alt={img.caption ?? img.filename}
                  style={{
                    width: "100%",
                    height: "auto",
                    border: `1px solid ${RULE}`,
                  }}
                />
                {img.caption && (
                  <figcaption
                    style={{
                      fontSize: "8.5pt",
                      color: MUTED,
                      marginTop: "3pt",
                    }}
                  >
                    {img.caption}
                  </figcaption>
                )}
              </figure>
            ))}
          </div>
        </div>
      )}
    </>
  );
}

function CommercialBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const c = data.commercial;
  return (
    <>
      <DataPanel
        items={[
          ["RRP", fmtMoney(c.rrp_cents)],
          ["OEM Price", fmtMoney(c.oem_price_cents)],
          [
            "Target GP",
            c.target_gp_pct != null ? `${c.target_gp_pct.toFixed(1)}%` : null,
          ],
        ]}
        emphasis
      />
      <Field label="Revenue Model" value={c.revenue_model} markdown />
      <Field label="Channel Strategy" value={c.channel_strategy} markdown />
      <Field label="Target Market" value={c.target_market} markdown />
      <Field label="Volume Assumptions" value={c.volume_assumptions} markdown />
    </>
  );
}

function DocumentsBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  if (data.documents.length === 0) {
    return <Empty />;
  }
  return (
    <DataTable
      headers={["Title", "Type", "Filename", "Notes", "Required Action"]}
      widths={["28%", "14%", "22%", "20%", "16%"]}
      rows={data.documents.map((d) => [
        d.title,
        cleanCell(d.doc_type),
        d.filename,
        cleanCell(d.notes),
        cleanCell(d.required_action),
      ])}
    />
  );
}

function ApprovalBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  const a = data.approval;
  return (
    <>
      <DataPanel
        items={[
          ["Status", statusLabel(a.status)],
          ["Reviewer", a.reviewer],
          ["Approver", a.approver],
        ]}
        breakWord
      />
      <DataPanel
        items={[
          [
            "Submitted",
            a.submitted_at ? new Date(a.submitted_at).toLocaleString() : null,
          ],
          [
            "Approved",
            a.approved_at ? new Date(a.approved_at).toLocaleString() : null,
          ],
        ]}
      />
      <Field label="Review Notes" value={a.review_notes} markdown />
      <Field label="Approval Notes" value={a.approval_notes} markdown />
    </>
  );
}

function ChangelogBody({ data }: { data: ExecSummaryDto }): JSX.Element {
  if (data.changelog.length === 0) {
    return <Empty />;
  }
  return (
    <DataTable
      headers={["Version", "Date", "Author", "Summary"]}
      widths={["12%", "16%", "30%", "42%"]}
      rows={data.changelog.map((e) => [
        e.version,
        e.changed_at,
        e.changed_by,
        e.summary,
      ])}
    />
  );
}

function DataPanel({
  items,
  emphasis = false,
  breakWord = false,
}: {
  items: ReadonlyArray<readonly [string, string | null | undefined]>;
  emphasis?: boolean;
  breakWord?: boolean;
}): JSX.Element {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${items.length}, 1fr)`,
        margin: "10pt 0",
        border: `1px solid ${RULE}`,
        background: PANEL,
        breakInside: "avoid",
      }}
    >
      {items.map(([k, v], i) => (
        <div
          key={k}
          style={{
            padding: "8pt 10pt",
            borderLeft: i === 0 ? "none" : `1px solid ${RULE}`,
            minWidth: 0,
          }}
        >
          <div
            style={{
              fontSize: "8pt",
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: MUTED,
              fontWeight: 600,
              marginBottom: "3pt",
            }}
          >
            {k}
          </div>
          <div
            style={{
              fontSize: emphasis ? "13pt" : "10pt",
              fontWeight: emphasis ? 600 : 400,
              color: v ? INK : FAINT,
              wordBreak: breakWord ? "break-word" : "normal",
              overflowWrap: breakWord ? "anywhere" : "normal",
            }}
          >
            {v || "—"}
          </div>
        </div>
      ))}
    </div>
  );
}

function Field({
  label,
  value,
  markdown = false,
}: {
  label: string;
  value: string | null | undefined;
  markdown?: boolean;
}): JSX.Element {
  return (
    <div style={{ marginBottom: "10pt", breakInside: "avoid" }}>
      <Label>{label}</Label>
      {value ? (
        markdown ? (
          <div style={{ marginTop: "3pt" }}>
            <PrintMarkdown>{value}</PrintMarkdown>
          </div>
        ) : (
          <div style={{ marginTop: "3pt", whiteSpace: "pre-wrap" }}>{value}</div>
        )
      ) : (
        <div style={{ marginTop: "3pt", color: FAINT }}>—</div>
      )}
    </div>
  );
}

function DataTable({
  headers,
  widths,
  rows,
}: {
  headers: readonly string[];
  widths?: readonly string[];
  rows: ReadonlyArray<readonly string[]>;
}): JSX.Element {
  return (
    <table
      style={{
        width: "100%",
        borderCollapse: "collapse",
        fontSize: "9.5pt",
        margin: "8pt 0",
        tableLayout: widths ? "fixed" : "auto",
      }}
    >
      <colgroup>
        {(widths ?? headers.map(() => undefined)).map((w, i) => (
          <col key={i} style={w ? { width: w } : undefined} />
        ))}
      </colgroup>
      <thead>
        <tr>
          {headers.map((h) => (
            <th
              key={h}
              style={{
                textAlign: "left",
                borderBottom: `1.5pt solid ${ACCENT}`,
                background: PANEL,
                padding: "6pt 8pt",
                fontWeight: 600,
                fontSize: "8.5pt",
                textTransform: "uppercase",
                letterSpacing: "0.06em",
                color: MUTED,
              }}
            >
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row, i) => (
          <tr
            key={i}
            style={{
              breakInside: "avoid",
              background: i % 2 === 1 ? PANEL : "transparent",
            }}
          >
            {row.map((cell, j) => (
              <td
                key={j}
                style={{
                  borderBottom: `0.5pt solid ${RULE}`,
                  padding: "5pt 8pt",
                  verticalAlign: "top",
                  wordBreak: "break-word",
                  overflowWrap: "anywhere",
                }}
              >
                {cell}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Label({ children }: { children: React.ReactNode }): JSX.Element {
  return (
    <div
      style={{
        fontSize: "8.5pt",
        textTransform: "uppercase",
        letterSpacing: "0.08em",
        color: MUTED,
        fontWeight: 600,
      }}
    >
      {children}
    </div>
  );
}

function Empty(): JSX.Element {
  return (
    <div style={{ color: FAINT, fontStyle: "italic", fontSize: "10pt" }}>
      None recorded.
    </div>
  );
}

function fmtMoney(cents: number | null | undefined): string | null {
  if (cents == null) return null;
  return `$${(cents / 100).toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`;
}

function statusLabel(s: string): string {
  return s === "in_review"
    ? "In Review"
    : s.charAt(0).toUpperCase() + s.slice(1);
}

function fmtDate(d: Date): string {
  return d.toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

function absoluteUrl(url: string): string {
  if (/^https?:\/\//.test(url)) return url;
  return `${window.location.origin}${url}`;
}

// Defensive: the legacy API sometimes serialised optional text fields
// as the literal string "undefined" instead of null. Map both to a
// proper em-dash for the print view.
function cleanCell(v: string | null | undefined): string {
  if (v == null) return "—";
  const t = v.trim();
  if (t === "" || t === "undefined" || t === "null") return "—";
  return t;
}
