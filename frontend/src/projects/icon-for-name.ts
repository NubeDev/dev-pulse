/**
 * Icon-for-name — pure, frontend-only mapping from a saved view's
 * name to a lucide icon component. Used by the project workbench's
 * `<ViewsTabStrip>` to give every tab a glanceable glyph without
 * adding an `icon` column to `dp_project_views`.
 *
 * The mapping is keyword-driven (substring, case-insensitive). The
 * first entry whose keywords match wins; the catch-all `Tag` icon
 * is returned when nothing matches so every name renders *some*
 * icon. Order matters — put the more specific rules first.
 */
import {
  AlertOctagonIcon,
  BoxesIcon,
  BugIcon,
  CalendarIcon,
  CheckCircle2Icon,
  ClipboardCheckIcon,
  CodeIcon,
  CpuIcon,
  FactoryIcon,
  FileTextIcon,
  FlagIcon,
  FlameIcon,
  GaugeIcon,
  HammerIcon,
  HandshakeIcon,
  HeadphonesIcon,
  LayersIcon,
  LightbulbIcon,
  ListChecksIcon,
  type LucideIcon,
  MegaphoneIcon,
  RocketIcon,
  SettingsIcon,
  ShieldCheckIcon,
  SparklesIcon,
  StarIcon,
  TagIcon,
  TargetIcon,
  TestTube2Icon,
  UsersIcon,
  WrenchIcon,
} from "lucide-react";

interface IconRule {
  icon: LucideIcon;
  /** Keywords matched case-insensitively as substrings against the
   *  view name (after stripping non-alphanum to spaces). */
  keywords: string[];
}

/** Ordered rule list — first match wins. The gate-specific rules
 *  (`g1` … `g8`) come *before* the generic `gate` / `milestone`
 *  fallback so each gate gets its own icon. Word boundaries are
 *  established by the normaliser which turns the name into a
 *  space-separated alphanum string. */
const RULES: IconRule[] = [
  // Gates G1–G8 — one icon per gate. Matched on the short code
  // (`g1`) or any of the gate's descriptive keywords so e.g.
  // "Executive Summary" still picks the G1 icon when the short
  // code isn't in the name.
  { icon: FileTextIcon, keywords: [" g1 ", "executive summary"] },
  { icon: TestTube2Icon, keywords: [" g2 ", "poc", "proof of concept"] },
  { icon: HammerIcon, keywords: [" g3 ", "mvp", "mvp build"] },
  { icon: HandshakeIcon, keywords: [" g4 ", "client acceptance", "acceptance"] },
  { icon: WrenchIcon, keywords: [" g5 ", "product refinement", "refinement", "polish"] },
  { icon: FactoryIcon, keywords: [" g6 ", "production ready", "production", "manufacturing"] },
  { icon: MegaphoneIcon, keywords: [" g7 ", "go to market", "gtm", "launch", "release", "ship"] },
  { icon: HeadphonesIcon, keywords: [" g8 ", "scale support", "scale and support", "scale"] },
  // Generic gate / milestone fallback.
  { icon: FlagIcon, keywords: ["gate", "milestone"] },
  // Generic launch / release (for non-G7 named views).
  { icon: RocketIcon, keywords: ["rollout"] },
  // Blocked / urgent
  { icon: AlertOctagonIcon, keywords: ["blocked", "stalled", "halt", "stuck"] },
  { icon: FlameIcon, keywords: ["urgent", "hot", "p0", "fire", "critical"] },
  // States
  { icon: CheckCircle2Icon, keywords: ["closed", "done", "complete"] },
  { icon: ListChecksIcon, keywords: ["status", "open", "todo", "backlog"] },
  { icon: ClipboardCheckIcon, keywords: ["review", "approve", "signoff", "qa"] },
  // Bugs / quality
  { icon: BugIcon, keywords: ["bug", "defect", "regression", "issue"] },
  { icon: ShieldCheckIcon, keywords: ["compliance", "security", "audit"] },
  // Categories
  { icon: CpuIcon, keywords: ["firmware", "hardware", "embedded", "device"] },
  { icon: CodeIcon, keywords: ["backend", "api", "code", "server"] },
  { icon: SettingsIcon, keywords: ["settings", "config", "infra", "build", "ops"] },
  { icon: LayersIcon, keywords: ["layer", "stack", "platform"] },
  { icon: BoxesIcon, keywords: ["category", "module", "component"] },
  // People / scale
  { icon: UsersIcon, keywords: ["team", "people", "client", "user"] },
  // Planning / measurement
  { icon: TargetIcon, keywords: ["goal", "objective", "target", "scope"] },
  { icon: GaugeIcon, keywords: ["perf", "performance", "speed", "metrics"] },
  { icon: CalendarIcon, keywords: ["sprint", "week", "month", "quarter", "schedule"] },
  { icon: StarIcon, keywords: ["priority", "starred", "important", "favourite", "favorite"] },
  { icon: LightbulbIcon, keywords: ["idea", "research", "spike", "discovery"] },
  { icon: SparklesIcon, keywords: ["new", "experiment"] },
];

const FALLBACK: LucideIcon = TagIcon;

/** Pick the best lucide icon for a view name. Pure + memo-friendly
 *  (no closures, no Date, no Math.random).
 *
 *  Normalisation wraps the alphanum-only form in spaces so rules
 *  can use ` g1 ` to match the short code as a whole word without
 *  also matching `g10`, `g1xyz`, etc. */
export function iconForName(name: string): LucideIcon {
  const norm = ` ${name.toLowerCase().replace(/[^a-z0-9]+/g, " ").trim()} `;
  for (const rule of RULES) {
    if (rule.keywords.some((k) => norm.includes(k))) {
      return rule.icon;
    }
  }
  return FALLBACK;
}

// ---------------------------------------------------------------------------
// Gate metadata — when a view name is the short gate code (`G1` …
// `G8`), the tab strip shows just the code and uses these to drive
// the hover tooltip and the icon's accent colour. Returns `null`
// for non-gate names so the caller can fall back to defaults.
// ---------------------------------------------------------------------------

interface GateMeta {
  /** Full gate label, shown as the button's `title` tooltip. */
  tooltip: string;
  /** Tailwind class applied to the leading lucide icon so each
   *  gate gets a distinct accent without dyeing the whole tab. */
  iconClass: string;
}

const GATE_META: Record<string, GateMeta> = {
  g1: { tooltip: "Executive Summary", iconClass: "text-sky-600 dark:text-sky-400" },
  g2: { tooltip: "Proof of Concept", iconClass: "text-violet-600 dark:text-violet-400" },
  g3: { tooltip: "MVP Build", iconClass: "text-amber-600 dark:text-amber-400" },
  g4: { tooltip: "Client Acceptance", iconClass: "text-pink-600 dark:text-pink-400" },
  g5: { tooltip: "Product Refinement", iconClass: "text-indigo-600 dark:text-indigo-400" },
  g6: { tooltip: "Production Ready", iconClass: "text-emerald-600 dark:text-emerald-400" },
  g7: { tooltip: "Go-To-Market", iconClass: "text-orange-600 dark:text-orange-400" },
  g8: { tooltip: "Scale & Support", iconClass: "text-teal-600 dark:text-teal-400" },
};

/** Return gate metadata if the view name *is* a gate short-code
 *  (`G1` … `G8`, case-insensitive, ignoring surrounding whitespace).
 *  Returns `null` otherwise — callers should treat that as "no
 *  special gate styling, render the name as-is". */
export function gateMetaForName(name: string): GateMeta | null {
  const key = name.trim().toLowerCase();
  return GATE_META[key] ?? null;
}

