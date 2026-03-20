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
  tracked_signal_slugs: string[];
};

export function releaseLabel(release: ReleaseRef, tagPrefix: string): string {
  const value = release.name?.trim() || release.tag_name;
  return value.startsWith(tagPrefix) ? value.slice(tagPrefix.length) : value;
}

export function trackedSignalsForDelta(
  delta: ReleaseDeltaData | null,
  signals: SignalCardData[],
): SignalCardData[] {
  if (!delta) {
    return [];
  }
  const bySlug = new Map(signals.map((signal) => [signal.slug, signal]));
  return delta.tracked_signal_slugs
    .map((slug) => bySlug.get(slug))
    .filter((signal): signal is SignalCardData => Boolean(signal));
}
