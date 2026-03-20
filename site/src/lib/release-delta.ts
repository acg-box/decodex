import type { SignalCardData } from "@/lib/signal-feed";

export type ReleaseRef = {
  tag_name: string;
  name: string;
  prerelease: boolean;
  published_at: string;
  url: string;
};

export type ReleaseDeltaData = {
  schema: "release_delta/v1";
  repo: string;
  tag_prefix: string;
  generated_at: string;
  stable_release: ReleaseRef;
  prerelease: ReleaseRef;
  compare: {
    status: string;
    ahead_by: number;
    total_commits: number;
    url: string;
    commit_shas: string[];
    pr_numbers: number[];
  };
  release_options: {
    stable: ReleaseRef[];
    preview: ReleaseRef[];
  };
  comparisons: Array<{
    stable_tag_name: string;
    prerelease_tag_name: string;
    compare: {
      status: string;
      ahead_by: number;
      total_commits: number;
      url: string;
      commit_shas: string[];
      pr_numbers: number[];
    };
    tracked_signal_slugs: string[];
  }>;
  tracked_signal_slugs: string[];
};

export type ReleaseComparisonData = ReleaseDeltaData["comparisons"][number];

export function releaseLabel(release: ReleaseRef, tagPrefix: string): string {
  const value = release.name?.trim() || release.tag_name;
  return value.startsWith(tagPrefix) ? value.slice(tagPrefix.length) : value;
}

export function trackedSignalsForSlugs(
  trackedSignalSlugs: string[],
  signals: SignalCardData[],
): SignalCardData[] {
  const bySlug = new Map(signals.map((signal) => [signal.slug, signal]));
  return trackedSignalSlugs
    .map((slug) => bySlug.get(slug))
    .filter((signal): signal is SignalCardData => Boolean(signal));
}

export function trackedSignalsForDelta(
  delta: ReleaseDeltaData | null,
  signals: SignalCardData[],
): SignalCardData[] {
  if (!delta) {
    return [];
  }
  return trackedSignalsForSlugs(delta.tracked_signal_slugs, signals);
}

export function comparisonKey(stableTagName: string, prereleaseTagName: string): string {
  return `${stableTagName}::${prereleaseTagName}`;
}

export function defaultComparison(delta: ReleaseDeltaData): ReleaseComparisonData {
  return (
    delta.comparisons.find(
      (comparison) =>
        comparison.stable_tag_name === delta.stable_release.tag_name &&
        comparison.prerelease_tag_name === delta.prerelease.tag_name,
    ) ?? delta.comparisons[0]
  );
}
