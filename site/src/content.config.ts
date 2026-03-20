import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

const sourceRefSchema = z
  .object({
    items: z
      .array(
        z.object({
          kind: z.enum(["pull_request", "commit"]),
          title: z.string().min(1),
          url: z.string().regex(/^https:\/\//, "source item url must be an https URL"),
          meta: z.string().min(1).optional(),
        }),
      )
      .default([]),
    repo: z.string().min(1),
    pr_url: z.string().regex(/^https:\/\//, "pr_url must be an https URL").optional(),
    commit_urls: z
      .array(z.string().regex(/^https:\/\//, "commit_urls entries must be https URLs"))
      .default([]),
  })
  .superRefine((entry, ctx) => {
    if (!entry.pr_url && entry.commit_urls.length === 0 && entry.items.length === 0) {
      ctx.addIssue({
        code: "custom",
        message: "source_refs must include a PR, commit URL, or titled source item.",
        path: ["commit_urls"],
      });
    }
  });

const signalEntrySchema = z
  .object({
    schema: z.literal("signal_entry/v1"),
    slug: z.string().min(1),
    lane: z.literal("github"),
    kind: z.enum(["capability", "behavior_change", "try_now"]),
    title: z.string().min(1),
    published_at: z.string().min(1),
    summary: z.string().min(1),
    why_it_matters: z.string().min(1),
    confidence: z.enum(["confirmed", "likely", "weak"]),
    impact: z.enum(["low", "medium", "high"]),
    config_flags: z.array(z.string()).default([]),
    how_to_try: z.string().min(1).optional(),
    expected_effect: z.string().min(1).optional(),
    proof_points: z.array(z.string().min(1)).min(1),
    source_refs: sourceRefSchema,
  })
  .superRefine((entry, ctx) => {
    const needsTryPath = entry.kind === "try_now" || entry.config_flags.length > 0;

    if (needsTryPath && !entry.how_to_try) {
      ctx.addIssue({
        code: "custom",
        message: "how_to_try is required for try_now entries and flag-backed entries.",
        path: ["how_to_try"],
      });
    }

    if (entry.how_to_try && !entry.expected_effect) {
      ctx.addIssue({
        code: "custom",
        message: "expected_effect is required when how_to_try is present.",
        path: ["expected_effect"],
      });
    }
  });

const releaseRefSchema = z.object({
  tag_name: z.string().min(1),
  name: z.string().min(1),
  prerelease: z.boolean(),
  published_at: z.string().min(1),
  url: z.string().regex(/^https:\/\//, "release url must be an https URL"),
});

const releaseDeltaSchema = z
  .object({
    schema: z.literal("release_delta/v1"),
    repo: z.string().min(1),
    tag_prefix: z.string().min(1),
    generated_at: z.string().min(1),
    stable_release: releaseRefSchema.extend({
      prerelease: z.literal(false),
    }),
    prerelease: releaseRefSchema.extend({
      prerelease: z.literal(true),
    }),
    compare: z.object({
      status: z.string().min(1),
      ahead_by: z.number().int().nonnegative(),
      total_commits: z.number().int().nonnegative(),
      url: z.string().regex(/^https:\/\//, "compare url must be an https URL"),
      commit_shas: z.array(z.string().min(1)).default([]),
      pr_numbers: z.array(z.number().int().positive()).default([]),
    }),
    tracked_signal_slugs: z.array(z.string().min(1)),
  })
  .superRefine((entry, ctx) => {
    if (!entry.stable_release.tag_name.startsWith(entry.tag_prefix)) {
      ctx.addIssue({
        code: "custom",
        message: "stable_release.tag_name must start with tag_prefix.",
        path: ["stable_release", "tag_name"],
      });
    }
    if (!entry.prerelease.tag_name.startsWith(entry.tag_prefix)) {
      ctx.addIssue({
        code: "custom",
        message: "prerelease.tag_name must start with tag_prefix.",
        path: ["prerelease", "tag_name"],
      });
    }
  });

const resetStatusSchema = z.object({
  schema: z.literal("reset_status/v1"),
  source_label: z.string().min(1),
  source_kind: z.literal("community"),
  source_url: z.string().regex(/^https:\/\//, "source_url must be an https URL"),
  source_api_url: z.string().regex(/^https:\/\//, "source_api_url must be an https URL"),
  status: z.enum(["reset", "not_reset", "unknown"]),
  stale: z.boolean(),
  configured: z.boolean(),
  upstream_state: z.string().min(1).nullable().optional(),
  auto_reset_hours: z.number().int().positive().nullable().optional(),
  reset_at: z.string().min(1).nullable().optional(),
  updated_at: z.string().min(1),
});

const signals = defineCollection({
  loader: glob({
    pattern: "**/*.json",
    base: "./src/content/signals",
  }),
  schema: signalEntrySchema,
});

const releaseDeltas = defineCollection({
  loader: glob({
    pattern: "**/*.json",
    base: "./src/content/release-deltas",
  }),
  schema: releaseDeltaSchema,
});

const resetStatuses = defineCollection({
  loader: glob({
    pattern: "**/*.json",
    base: "./src/content/reset-status",
  }),
  schema: resetStatusSchema,
});

export const collections = {
  signals,
  releaseDeltas,
  resetStatuses,
};
