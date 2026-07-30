---
name: x-post-quality-system
description: Use when Decodex Xurl Publisher needs to decide whether an artifact-backed @decodexspace post is worth publishing.
---

# Decodex X Post Quality System

Use before `x-post-publisher` decides whether a `social_candidate/v1` or explicit
operator handoff is worth publishing. This skill raises the bar from source-backed
correctness to public-reader value.

## Read First

- `../references/social-release-publisher-gates.md`

## Hard Boundaries

- Do not read upstream Codex source or invent missing technical evidence.
- Accept a publish candidate only after `radar content-eligibility` returns one
  `radar_content_eligibility/v1` receipt for the exact private queue, review, and
  impact files. Re-read and verify those files through Publisher.
- Keep weak candidates as `decision.worthiness = "skip"`, or write a terminal
  `skipped`/`blocked` `social_post/v1` when Publisher has already started.
- CodexRadar and public release sources are editorial benchmarks only. Content
  Manager may inspect them through ordinary web research. It must not spend X API
  budget, copy text, or use their coverage as technical evidence.

## Editorial Gate

Publish only when the candidate answers all three questions in one screen:

- What changed?
- Who can use, observe, or act on it?
- What source proves it?

Reject candidates that are only single-PR renames, trace-only compatibility notes,
narrow bug/fix details, Decodex-internal audit targets, low-context cautions, or generic
"watching this" notes. They may remain Radar artifacts, but should not become X posts
unless they roll up into a broader release, product update, concrete workflow change, or
external operator decision.

For prerelease coverage, reject generic theme prose when compare metadata contains named
PRs or commit titles. The draft must surface at least two of: important PR/commit
clusters, anticipated user workflow changes, protocol/API/schema changes, removals or
compatibility boundaries, and plugin/config/sandbox/tool/release-engineering changes.

Formatting is part of the quality bar. Reject dense paragraphs when bullets or blank
lines would improve scanning. Keep direct PR URLs in private evidence, not in public
post text.

The public text must be a canonical ordered claim composition. Each claim segment identifies one
ordered `claims` entry, and each claim binds one verified Radar review or impact.
Only schema-allowlisted non-factual connective segments can appear between claims.
Reject added factual clauses, release assertions, dates, availability statements,
or consequences that do not exist as an evidence-bound claim.

Reject post bodies that start with or repeat automation attribution such as
`Automated by @hackink`. Attribution belongs in durable records and account metadata,
not in the reader-facing post text. Also reject cramped post bodies longer than 260
characters. Public source URLs and multi-post threads are not supported. Skip the
candidate when its useful claim cannot fit one clear post. A good post should leave
room for quote metadata and minor manual edits without hitting the hard platform
limit.

Prefer concrete evidence over generic release prose. A publishable post should name
the release, PR, commit cluster, protocol surface, workflow, or operator action that
matters. If the best available text is only "tracking", "watching", or "new release
available" without a reader action or source-backed implication, mark the candidate as
`skip`.

The xurl route is text-only. Reject candidates that require media to be useful or
correct. Also verify the candidate is not already live in validated local publication
records and does not conflict with an active `social_publish_reservation/v1`.
Content Manager writes only a private staging artifact. The Publisher
`social record-manager` command is the only candidate or strategy store writer.
