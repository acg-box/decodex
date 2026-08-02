# Radar

Radar is a standalone auxiliary tool for bounded GitHub evidence, Codex release
deltas, static signals, and private cache validation. It is optional research
input for Decodex agents and has no scheduled automation role.

```sh
radar --help
```

## Current Surface

```text
radar validate [PATH ...]
radar cache-gc
radar refresh-upstream-queue
radar refresh-release-delta
radar bundle ...
radar render-signal ...
radar backfill-release-range ...
radar ledger ...
```

The removed content selection, pair, eligibility, and reset commands have no
replacement. An autonomous agent researches official sources directly and sends
one source-backed content decision to Decodex Publisher.

## GitHub Authentication

GitHub-backed commands resolve a token in this order:

1. an explicit `--token-env` variable, which must be present and non-empty;
2. the repository-routed identity variable;
3. `GH_TOKEN`;
4. `GITHUB_TOKEN`.

Radar sends credentials only to the exact HTTPS GitHub API origin. Pagination,
response size, item count, retries, and timeout are bounded.

## Bundles And Queues

`radar bundle build` derives its lowercase UUID from `CODEX_THREAD_ID`, writes
one deterministic `github_change_bundle/v1` into the private cache, reads the
exact bytes back, and returns one bounded `radar_bundle_build_receipt/v1`.
The receipt contains SHA-256, byte count, analysis mode, and structural counts.
It does not contain patch text, credentials, source identity, or local paths.

`radar refresh-upstream-queue` records a deterministic GitHub discovery snapshot
with schema `upstream_review_queue/v1`. Queue metadata is triage evidence only.
It cannot make a compatibility or publication decision.

`radar refresh-release-delta` records current release evidence for static
analysis. `radar render-signal` produces reviewed static signal entries. These
artifacts can support research, but Publisher consumes direct source URLs rather
than private Radar lineage.

## Cache Safety

Generated data stays under `.agent/automations/radar/cache`. Directories are mode
`0700`; JSON and SQLite files are mode `0600`.

`radar validate` with no path is the normal health gate. It runs retention,
requires current canonical queue and release snapshots, and validates supported
artifacts. `radar validate --bootstrap` accepts only a completely empty generated
cache. An explicit missing path is an error.

`radar cache-gc` applies bounded age, count, and byte limits to active Radar
collections and the disposable ledger. One cache lock serializes writers and
retention. Descriptor-relative no-follow traversal rejects symlink ancestors,
`..`, wrong owner or mode, unexpected hard links, and identity replacement.
Writes use atomic replacement and exact readback.

## Separation From Publisher

Radar does not own Decodex social candidates, X decisions, reservations, posts,
outcomes, xurl authorization, or cost state. It does not invoke xurl. There is no
cache-path handoff between Radar and Publisher.

CodexRadar can inform an agent's editorial comparison, but it is secondary
evidence. A public factual claim still needs an official Codex or landed Decodex
source.

## Validation

```sh
cargo test -p radar
cargo clippy -p radar --all-features --all-targets -- -D warnings
```
