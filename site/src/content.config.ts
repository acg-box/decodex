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
    caveats: z.array(z.string().min(1)).default([]),
    watch_state: z.string().min(1).optional(),
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

const compareSummarySchema = z.object({
  status: z.string().min(1),
  ahead_by: z.number().int().nonnegative(),
  total_commits: z.number().int().nonnegative(),
  url: z.string().regex(/^https:\/\//, "compare url must be an https URL"),
  commit_shas: z.array(z.string().min(1)).default([]),
  pr_numbers: z.array(z.number().int().positive()).default([]),
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
    compare: compareSummarySchema,
    release_options: z.object({
      stable: z.array(releaseRefSchema.extend({ prerelease: z.literal(false) })).min(1),
      preview: z.array(releaseRefSchema.extend({ prerelease: z.literal(true) })).min(1),
    }),
    comparisons: z.array(z.object({
      stable_tag_name: z.string().min(1),
      prerelease_tag_name: z.string().min(1),
      compare: compareSummarySchema,
      tracked_signal_slugs: z.array(z.string().min(1)),
    })),
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
    const stableTags = new Set(entry.release_options.stable.map((release) => release.tag_name));
    const previewTags = new Set(entry.release_options.preview.map((release) => release.tag_name));
    const hasDefaultComparison = entry.comparisons.some(
      (comparison) =>
        comparison.stable_tag_name === entry.stable_release.tag_name &&
        comparison.prerelease_tag_name === entry.prerelease.tag_name,
    );
    if (!hasDefaultComparison) {
      ctx.addIssue({
        code: "custom",
        message: "comparisons must include the default stable/prerelease pair.",
        path: ["comparisons"],
      });
    }
    entry.comparisons.forEach((comparison, index) => {
      if (!stableTags.has(comparison.stable_tag_name)) {
        ctx.addIssue({
          code: "custom",
          message: "comparison stable_tag_name must exist in release_options.stable.",
          path: ["comparisons", index, "stable_tag_name"],
        });
      }
      if (!previewTags.has(comparison.prerelease_tag_name)) {
        ctx.addIssue({
          code: "custom",
          message: "comparison prerelease_tag_name must exist in release_options.preview.",
          path: ["comparisons", index, "prerelease_tag_name"],
        });
      }
    });
  });

const resetStatusEvidencePostSchema = z.object({
  published_at_label: z.string().min(1).optional(),
  relevance: z.enum(["related", "not_related", "uncertain"]),
  summary: z.string().min(1),
  url: z.string().regex(/^https:\/\//, "post url must be an https URL").optional(),
});

const resetStatusSchema = z.object({
  schema: z.literal("reset_status/v1"),
  question: z.string().min(1),
  answer: z.enum(["yes", "no", "unknown"]),
  confidence: z.enum(["confirmed", "likely", "weak"]),
  observed_for_date: z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "observed_for_date must be YYYY-MM-DD"),
  timezone: z.string().min(1),
  generated_at: z.string().min(1),
  source_account: z.string().min(1),
  source_url: z.string().regex(/^https:\/\//, "source_url must be an https URL"),
  search_url: z.string().regex(/^https:\/\//, "search_url must be an https URL").optional(),
  judgment_mode: z.literal("ai_semantic_review"),
  rationale: z.string().min(1),
  evidence_posts: z.array(resetStatusEvidencePostSchema).default([]),
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

const resetStatus = defineCollection({
  loader: glob({
    pattern: "**/*.json",
    base: "./src/content/reset-status",
  }),
  schema: resetStatusSchema,
});

export const collections = {
  signals,
  releaseDeltas,
  resetStatus,
};
