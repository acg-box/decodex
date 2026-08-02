# Radar Research Assets

This directory contains reusable assets for the standalone Radar auxiliary tool.
Radar has no native schedule in the exact-five Decodex automation portfolio.

- `radar.toml` declares only Radar-owned private cache paths.
- `scripts/github/` contains bounded GitHub collection and analysis helpers.
- `skills/` contains optional research skills for upstream triage, code analysis,
  release analysis, and static signal drafting.

The Maintainer and Content Manager may use Radar as secondary research input.
They are not required to use it, and Radar output never authorizes repository or
X mutations.

Generated Radar state belongs under `.agent/automations/radar/cache`. It is
owner-only local data and must not be uploaded. Radar cache and Publisher cache
have no path handoff contract.

Radar owns deterministic GitHub bundles, review queues, release deltas, static
signals, validation, retention, and its disposable ledger. It no longer owns a
Content Manager queue, review pair, eligibility gate, activation/reset path, or
Publisher workflow.

Use the current CLI surface:

```sh
radar refresh-upstream-queue
radar refresh-release-delta
radar bundle build --help
radar render-signal --help
radar validate
radar cache-gc
```

The first-run validation contract accepts only an empty generated cache with
`radar validate --bootstrap`. Normal validation requires current canonical
artifacts. Cache directories are mode `0700`; files are mode `0600`. Radar
rejects symlink traversal, wrong ownership or mode, unexpected hard links, and
path replacement.

For content automation, an agent cites the original official URL in
`decodex/content-evidence/1`. A Radar or CodexRadar URL can be additional
`radar_secondary` evidence. No private Radar artifact is copied into Publisher
state.
