import featureCatalog from "@/generated/codex-config-features.json";

type FeatureCatalogEntry = {
  name: string;
  config_path: string;
  toml_assignment: string;
  toml_snippet: string;
  cli_enable_flag: string;
  schema_url: string;
  reference_url: string;
  reference_description?: string | null;
  github_search_url: string;
};

type FeatureCatalog = {
  schema: "codex_config_feature_catalog/v1";
  source_url: string;
  generated_at: string;
  feature_count: number;
  features: FeatureCatalogEntry[];
};

export type ResolvedConfigFlag = {
  raw: string;
  display: string;
  kind: "feature" | "raw";
  configPath?: string;
  cliEnableFlag?: string;
  schemaUrl?: string;
  referenceUrl?: string;
  referenceDescription?: string | null;
  githubSearchUrl?: string;
};

const catalog = featureCatalog as FeatureCatalog;
const featureByName = new Map(catalog.features.map((feature) => [feature.name, feature]));

const ENABLE_FEATURE_RE = /^--enable\s+([a-z0-9_]+)$/i;
const FEATURE_PATH_RE = /^(?:features\.)?([a-z0-9_]+)(?:\s*=\s*true)?$/i;

function resolveFeatureName(raw: string): string | null {
  const trimmed = raw.trim();
  const enableMatch = trimmed.match(ENABLE_FEATURE_RE);
  if (enableMatch) {
    const name = enableMatch[1].toLowerCase();
    return featureByName.has(name) ? name : null;
  }

  const featureMatch = trimmed.match(FEATURE_PATH_RE);
  if (featureMatch) {
    const name = featureMatch[1].toLowerCase();
    return featureByName.has(name) ? name : null;
  }

  return null;
}

export function resolveFeatureToggleByName(name: string): ResolvedConfigFlag | null {
  const normalized = name.trim().toLowerCase();
  const feature = featureByName.get(normalized);
  if (!feature) return null;
  return {
    raw: feature.config_path,
    display: `${feature.config_path} = true`,
    kind: "feature",
    configPath: feature.config_path,
    cliEnableFlag: feature.cli_enable_flag,
    schemaUrl: feature.schema_url,
    referenceUrl: feature.reference_url,
    referenceDescription: feature.reference_description,
    githubSearchUrl: feature.github_search_url,
  };
}

export function resolveConfigFlags(rawFlags: string[]): ResolvedConfigFlag[] {
  const resolved: ResolvedConfigFlag[] = [];
  const seen = new Set<string>();

  for (const raw of rawFlags) {
    const featureName = resolveFeatureName(raw);
    if (featureName) {
      const feature = featureByName.get(featureName);
      if (!feature) continue;
      const dedupeKey = `feature:${feature.config_path}`;
      if (seen.has(dedupeKey)) continue;
      seen.add(dedupeKey);
      resolved.push({
        raw,
        display: `${feature.config_path} = true`,
        kind: "feature",
        configPath: feature.config_path,
        cliEnableFlag: feature.cli_enable_flag,
        schemaUrl: feature.schema_url,
        referenceUrl: feature.reference_url,
        referenceDescription: feature.reference_description,
        githubSearchUrl: feature.github_search_url,
      });
      continue;
    }

    const trimmed = raw.trim();
    const actionable = trimmed.startsWith("--") || trimmed.includes("=");
    if (!actionable) continue;

    const dedupeKey = `raw:${trimmed}`;
    if (seen.has(dedupeKey)) continue;
    seen.add(dedupeKey);
    resolved.push({
      raw: trimmed,
      display: trimmed,
      kind: "raw",
    });
  }

  return resolved;
}
