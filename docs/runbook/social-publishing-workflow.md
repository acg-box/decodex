# Social Publishing Workflow

Goal: Turn Radar evidence into reviewable `@decodexspace` social drafts without making
the public site or X account depend on a live Decodex daemon.

Read this when:
- You are preparing X posts about Codex releases, PRs, app updates, or usage patterns.
- You need to decide whether a Decodex signal should also produce a social draft.
- You are reviewing a `social_post_draft/v1` before external publication.

Inputs:
- Source evidence from GitHub, OpenAI developer changelogs, checked-in signal entries,
  release-delta artifacts, or verified browser observations.
- The governing schemas:
  - [`../spec/upstream-impact.md`](../spec/upstream-impact.md)
  - [`../spec/social-post-draft.md`](../spec/social-post-draft.md)
  - [`../spec/signal-entry.md`](../spec/signal-entry.md)

Depends on:
- [`local-github-signal-workflow.md`](./local-github-signal-workflow.md) for the
  GitHub signal path.
- [`../../dev/skills/x-post-draft/SKILL.md`](../../dev/skills/x-post-draft/SKILL.md)
  for the repo-local drafting method.
- [`../decisions/radar-control-plane-publisher.md`](../decisions/radar-control-plane-publisher.md)
  for the Radar, Control Plane, and Publisher boundary.
- [`../decisions/static-public-site.md`](../decisions/static-public-site.md) for the
  static-first public surface decision.

Outputs:
- An optional `upstream_impact/v1` artifact under `artifacts/github/impact/`.
- An optional `social_post_draft/v1` artifact under `artifacts/social/x/`.
- A published X URL only after explicit approval.

## Style Benchmarks

These benchmark observations are for tone and format only. They are not source evidence
for technical claims.

| Account | Useful pattern | Decodex stance |
| --- | --- | --- |
| `@Codex_Changelog` | Fast release-aware bullets with a changelog link. | Useful for `release_pulse`, but Decodex should not become a duplicate release bot. |
| `@LLMJunky` | Practical user interpretation: how a feature changes real workflows, what is worth trying, and where limits remain. | Prefer this style when Radar evidence can support the claim quickly. |
| `@decodexspace` | Fresh account with no post history yet. | Establish a voice around evidence-backed Codex intelligence and Decodex operator impact. |

## Workflow

1. Start from source evidence.
   - Prefer a merged PR bundle, release note, OpenAI developer changelog entry, or
     already-rendered `signal_entry/v1`.
   - Do not start from social engagement alone.

2. Classify upstream impact.
   - Write or update `artifacts/github/impact/<slug>.json` when the change may affect
     Control Plane or Publisher.
   - Use `public_signal_decision`, `control_plane_impact`, and `publisher_angle` from
     [`../spec/upstream-impact.md`](../spec/upstream-impact.md).

3. Decide whether to draft a post.
   - Draft when the change has a clear `release_pulse`, `practical_explainer`,
     `operator_impact`, or `watch_note` angle.
   - Skip when the change is internal cleanup, too weakly sourced, too private, or too
     vague for a useful reader takeaway.

4. Create a checked-in draft.
   - Use `dev/skills/x-post-draft/SKILL.md`.
   - Write `artifacts/social/x/<slug>.json`.
   - Use `schema = "social_post_draft/v1"`.
   - Keep `status = "draft"` until approval.
   - Keep `text[]` short enough for X, one item per post in a thread.

5. Review the claims.
   - Every user-facing claim must map to source evidence.
   - Confirm the post does not imply shipped Decodex behavior without Control Plane
     evidence.
   - Confirm beta, rollout, platform, and config caveats are explicit.

6. Approve or reject.
   - Move `status` to `approved` only after human or explicitly routed approval.
   - Keep rejected drafts as `status = "rejected"` when the rejection explains a useful
     future boundary.

7. Publish externally.
   - Do not post from automation unless the draft is already `approved`.
   - After posting, update the artifact to `status = "published"` and set
     `published_url`.

## Mode Guidance

Use `release_pulse` when:

- the release note itself is the story
- the post is mainly fast awareness
- the change does not yet justify a deeper Decodex angle

Use `practical_explainer` when:

- a reader can try the change in one short session
- the expected result is observable
- the value is easier to understand through workflow language than release bullets

Use `operator_impact` when:

- the change touches app-server, plugins, browser automation, MCP, permissions,
  sandboxing, config, or runtime orchestration
- Decodex Control Plane may need to adopt, watch, or guard against the change
- the public explanation can stay honest about what Decodex has and has not shipped

Use `watch_note` when:

- the change is interesting but evidence is incomplete
- rollout or platform status is unclear
- a strong recommendation would overclaim

## Guardrails

- Do not send credentials, private issue details, or local runtime paths to X.
- Do not publish unapproved drafts.
- Do not use `@Chrome` or any browser automation to post externally without explicit
  user approval for that specific post.
- Do not let social drafting bypass the static site, signal-entry, or upstream-impact
  evidence chain.
- Do not quote third-party posts at length. Record style observations, not copied
  content.
