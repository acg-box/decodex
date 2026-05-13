---
name: x-post-draft
description: Use when turning Decodex Radar evidence, upstream-impact classifications, signal entries, release analysis, or verified browser style observations into a checked-in social_post_draft/v1 artifact for X.
---

# Decodex X Post Draft

Use this skill after source evidence exists. Its job is to create reviewable X draft
artifacts, not to publish posts.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Read Before Drafting

- `docs/spec/social-post-draft.md`
- `docs/spec/upstream-impact.md`
- `docs/runbook/social-publishing-workflow.md`
- `dev/skills/codex-release-analysis/SKILL.md`
- `dev/skills/codex-code-analysis/SKILL.md`

## Inputs

- `signal_entry/v1`, `upstream_impact/v1`, release-analysis note, or checked source URLs
- Optional style observations from `@Codex_Changelog`, `@LLMJunky`, or `@decodexspace`
- Target account, normally `decodexspace`

## Browser Boundary

Use `@Chrome` only for reading public pages or verifying rendered posts. Do not type
into an X composer, save a web draft, or post externally unless the user explicitly
approves the specific draft and the artifact is already `status = "approved"`.

Style observations from X are not technical evidence. They can shape format and tone,
but every technical claim must point back to GitHub, changelog, signal, or
upstream-impact evidence.

## Benchmark Patterns

Use these as format patterns only:

| Pattern | Good for | Decodex adaptation |
| --- | --- | --- |
| Release-bot bullet | Fast `release_pulse` posts. | Version or source headline, two or three evidence-backed bullets, source link. |
| Human workflow read | `practical_explainer` and `operator_impact`. | Start with the concrete workflow change, then explain why it matters and what caveat remains. |
| Watch note | Interesting but incomplete evidence. | Say what changed, why Radar is watching, and what evidence is still missing. |

## Draft Modes

Choose exactly one `mode` from `social_post_draft/v1`:

- `release_pulse`: short release-aware summary with source link.
- `practical_explainer`: concrete user workflow and expected result.
- `operator_impact`: Decodex Control Plane implication.
- `thread`: multi-post explanation when one post hides evidence or caveats.
- `watch_note`: cautious public note for incomplete evidence.

`@decodexspace` should mostly use `practical_explainer` and `operator_impact`.
Use `release_pulse` only when the release itself is the useful alert.

## Claim Review

Before writing the artifact:

- Map every sentence to evidence.
- Remove claims based only on social posts or engagement.
- Make beta, rollout, platform, and config gates explicit.
- Avoid local paths, credentials, private issue details, or internal runtime state.
- Keep each `text[]` item within the X length limit.

## Output

Write or propose `artifacts/social/x/<slug>.json` with:

- `schema = "social_post_draft/v1"`
- `channel = "x"`
- `target_account = "decodexspace"` unless requested otherwise
- `status = "draft"`
- `source_refs`, `evidence_notes`, and `claims`
- `caveats` when confidence is not fully confirmed

Do not update the artifact to `approved` or `published` from this skill unless the user
explicitly asks for that state change.
