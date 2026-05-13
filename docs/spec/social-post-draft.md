# Social Post Draft

Purpose: Define the checked-in draft artifact used before Decodex publishes from the
`@decodexspace` X account or another external social channel.

Status: normative

Read this when:
- You are generating, reviewing, or validating social publishing drafts.
- You need to decide what evidence a post must carry before external publication.
- You are extending Publisher beyond static site signal entries.

Not this document:
- The upstream GitHub bundle schema. Read [`github-change-bundle.md`](./github-change-bundle.md).
- The public site signal-entry schema. Read [`signal-entry.md`](./signal-entry.md).
- The social publishing procedure. Read
  [`../runbook/social-publishing-workflow.md`](../runbook/social-publishing-workflow.md).

Defines:
- The `social_post_draft/v1` artifact shape.
- Allowed post modes for Decodex Publisher.
- Review and publication state rules.

## Artifact identity

The canonical schema identifier is:

- `social_post_draft/v1`

Recommended checked-in location:

- `artifacts/social/x/<slug>.json`

## Required fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `social_post_draft/v1`. |
| `slug` | string | Stable URL-safe identifier for the draft. |
| `channel` | string | Must be `x` for X/Twitter drafts. |
| `target_account` | string | Account handle without URL, such as `decodexspace`. |
| `mode` | string | One value from the post-mode table. |
| `status` | string | `draft`, `approved`, `published`, or `rejected`. |
| `audience` | string | Primary reader group. |
| `text` | array | One or more post bodies, one array item per thread post. |
| `source_refs` | object | Links to signal, upstream-impact, release, PR, or changelog evidence. |
| `evidence_notes` | array | Non-empty list of evidence-backed notes that justify the post. |
| `claims` | array | Non-empty list of user-facing claims with evidence references. |

Optional fields:

- `published_url`: required when `status = "published"`.
- `approval`: reviewer, timestamp, and notes when `status = "approved"` or
  `status = "published"`; optional rejection notes when `status = "rejected"`.
- `caveats`: rollout limits, uncertainty, platform limits, or version gates.
- `media_refs`: checked-in screenshots, videos, or generated assets used by the post.

## Post modes

Use exactly one `mode` value:

| Value | Purpose |
| --- | --- |
| `release_pulse` | Short release-aware summary with a source link. |
| `practical_explainer` | Concrete user-facing explanation of how to try or reason about a feature. |
| `operator_impact` | Decodex-specific explanation of app-server, plugin, browser, MCP, sandbox, config, or orchestration implications. |
| `thread` | Multi-post explanation when one post would hide important evidence or caveats. |
| `watch_note` | Cautious note for interesting changes that are not ready for a strong recommendation. |

`release_pulse` should be the minority path for `@decodexspace`; the account should
differ from release-only bots by preferring `practical_explainer` and `operator_impact`
drafts when evidence supports them.

## Claim rules

Each `claims[]` entry must include:

- `text`: the claim visible or implied in the post.
- `evidence`: source reference key, URL, file path, or artifact path.
- `confidence`: `confirmed`, `likely`, or `weak`.

Rules:

- Do not publish a claim without evidence.
- Do not imply Decodex runtime support unless Control Plane evidence exists.
- Do not present a beta, hidden, or rollout-gated capability as generally available.
- Do not use a social post to replace the site signal or upstream-impact artifact.
- Do not quote third-party posts at length. Summarize style or public reaction unless
  the quoted text is short and necessary.

## Status rules

- `draft`: generated or edited, not approved for external publication.
- `approved`: reviewed by a human or an explicitly routed approval process.
- `published`: externally posted; `published_url` is required.
- `rejected`: intentionally not publishable; keep rejection notes in `approval.notes`
  or `caveats`.

No automation may post a `draft` directly to X. External publication requires
`status = "approved"` immediately before the posting action.
