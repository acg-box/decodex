export const FILTERS = [
  {
    id: "all",
    label: "All",
    description: "Everything in the current feed.",
  },
  {
    id: "github",
    label: "GitHub",
    description: "PR-first GitHub change signals.",
  },
  {
    id: "try-now",
    label: "Try Now",
    description: "Signals with an actionable try path.",
  },
  {
    id: "high-impact",
    label: "High Impact",
    description: "Directional or high-value shifts worth watching.",
  },
] as const;

export type FilterId = (typeof FILTERS)[number]["id"];
export type SignalKind = "capability" | "behavior_change" | "try_now";
export type SignalImpact = "low" | "medium" | "high";
export type SignalConfidence = "confirmed" | "likely" | "weak";

export type SourceRefItem = {
  kind: "pull_request" | "commit";
  title: string;
  url: string;
  meta?: string;
};

export type SourceRefs = {
  items?: SourceRefItem[];
  repo: string;
  pr_url?: string;
  commit_urls: string[];
};

export type SignalCardData = {
  id: string;
  schema: "signal_entry/v1";
  slug: string;
  lane: "github";
  kind: SignalKind;
  title: string;
  published_at: string;
  summary: string;
  why_it_matters: string;
  confidence: SignalConfidence;
  impact: SignalImpact;
  config_flags: string[];
  caveats: string[];
  watch_state?: string;
  proof_points: string[];
  source_refs: SourceRefs;
  how_to_try?: string;
  expected_effect?: string;
  previewLabel?: string;
};

export type SignalGroup = {
  id: string;
  label: string;
  items: SignalCardData[];
};

const DEPRECATED_OR_MIGRATION_PATTERN =
  /\b(deprecat|remove|removed|drops?|no longer|legacy|disabled|disable|rollback|rolled back|breaking)\b/i;

export function isFilterId(value: string | null): value is FilterId {
  return FILTERS.some((filter) => filter.id === value);
}

export function filterSignals(signals: SignalCardData[], filter: FilterId): SignalCardData[] {
  switch (filter) {
    case "all":
      return signals;
    case "github":
      return signals.filter((signal) => signal.lane === "github");
    case "try-now":
      return signals.filter(
        (signal) => signal.kind === "try_now" || Boolean(signal.how_to_try),
      );
    case "high-impact":
      return signals.filter((signal) => signal.impact === "high");
  }
}

export function sortSignals(signals: SignalCardData[]): SignalCardData[] {
  return [...signals].sort((left, right) =>
    right.published_at.localeCompare(left.published_at),
  );
}

export function isDeprecatedOrMigrationSignal(signal: SignalCardData): boolean {
  const searchable = [
    signal.title,
    signal.summary,
    signal.why_it_matters,
    signal.watch_state ?? "",
    ...signal.caveats,
  ].join("\n");
  return DEPRECATED_OR_MIGRATION_PATTERN.test(searchable);
}

export function isHomepageSignal(signal: SignalCardData): boolean {
  if (signal.impact !== "low") return true;
  if (signal.kind === "try_now") return true;
  if (signal.how_to_try) return true;
  if (signal.config_flags.length > 0) return true;
  if (signal.kind === "capability" && signal.confidence === "confirmed") return true;
  return isDeprecatedOrMigrationSignal(signal);
}

export function homepageSignals(signals: SignalCardData[]): SignalCardData[] {
  return sortSignals(signals).filter(isHomepageSignal);
}

export function groupSignalsByMonth(signals: SignalCardData[]): SignalGroup[] {
  const groups = new Map<string, SignalCardData[]>();

  for (const signal of signals) {
    const date = new Date(signal.published_at);
    const key = `${date.getUTCFullYear()}-${String(date.getUTCMonth() + 1).padStart(2, "0")}`;
    const existing = groups.get(key);

    if (existing) {
      existing.push(signal);
    } else {
      groups.set(key, [signal]);
    }
  }

  return Array.from(groups.entries()).map(([id, items]) => ({
    id,
    label: formatMonthGroup(items[0]?.published_at ?? `${id}-01T00:00:00Z`),
    items,
  }));
}

export function formatPublishedAt(value: string): string {
  return new Intl.DateTimeFormat("en", {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(new Date(value));
}

export function formatMonthGroup(value: string): string {
  return new Intl.DateTimeFormat("en", {
    month: "long",
    year: "numeric",
  }).format(new Date(value));
}

export function kindLabel(kind: SignalKind): string {
  switch (kind) {
    case "capability":
      return "Capability";
    case "behavior_change":
      return "Behavior Change";
    case "try_now":
      return "Try Now";
  }
}

export function impactLabel(impact: SignalImpact): string {
  switch (impact) {
    case "low":
      return "Low Impact";
    case "medium":
      return "Medium Impact";
    case "high":
      return "High Impact";
  }
}

export function confidenceLabel(confidence: SignalConfidence): string {
  switch (confidence) {
    case "confirmed":
      return "Confirmed";
    case "likely":
      return "Likely";
    case "weak":
      return "Weak";
  }
}
