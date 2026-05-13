# Site Contract

Purpose: Define the page and route contract for the GitHub-first Decodex MVP.

Status: normative

Read this when:
- You are scaffolding the static site.
- You are implementing routes, homepage sections, or feed filters.
- You need to know what the MVP page count is allowed to be.

Not this document:
- The GitHub change-bundle schema.
- The published signal-entry schema.
- The local editorial workflow.

Defines:
- Allowed public routes.
- Required homepage sections.
- Required filter set.
- The minimum information that a signal card must expose.

## Route budget

The MVP route budget is:

- Required: `/`
- Optional: exactly one secondary public route

The secondary route may be either:

- an archive route, or
- a per-signal detail route

The MVP must not introduce multiple parallel content sections such as separate public `signals`, `shiplog`, `notes`, and `tools` pages.

## Homepage obligations

The homepage is the primary product surface. It must contain:

- a short positioning line that defines Decodex as a signal layer
- a compact brand treatment that outweighs the supporting copy
- a compact release-delta module when a valid release-delta artifact exists
- a lightweight filter bar
- the primary signal feed
- an optional compact utility slot that does not dominate the page

The homepage must remain scan-first. Large marketing hero sections, dashboard-style multi-column panels, and documentation-style navigation trees are out of scope for the MVP.

The release-delta module must summarize the latest stable release, the latest prerelease, and the tracked signal differences unlocked by the prerelease without displacing the primary feed.

The primary feed is curated for community-ready signals, not every analyzed upstream
commit. Low-impact internal changes without a try path, capability value, or
deprecated/migration cue may stay in the signal collection, Radar ledger, or release
rollup inputs without appearing in the homepage feed.

When the latest stable-to-prerelease pair has no matching published public signals, the
homepage may default the comparator to the most recent signal-bearing pair while keeping
the latest pair visible in the comparator options.

## Allowed filters

The MVP filter set is:

- `all`
- `github`
- `try-now`
- `high-impact`

Additional filters require a plan update.

## Signal-card rendering contract

Every rendered signal card must surface these fields without requiring a click:

- `title`
- `published_at`
- `impact`
- `confidence`
- `summary`
- `why_it_matters`

Every rendered signal card must also expose one of these action states:

- `how_to_try`, or
- an explicit watch-only state when no safe try path exists

Source references must be reachable from the card or its immediate expansion state.

## Secondary route rule

If the MVP includes a secondary route, the homepage remains the primary entry point and the secondary route must add depth without becoming a parallel information architecture.
