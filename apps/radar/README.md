# Radar

Radar is the Decodex auxiliary automation tool for upstream review queues,
release deltas, artifact validation, signal rendering, and bundle generation.

Run it with:

```sh
radar --help
```

```sh
radar validate .agent/automations/radar/cache/site-content/signals
```

GitHub-backed commands resolve authentication in this order:

1. An explicit `--token-env` value. The command fails if that variable is missing or empty.
2. `GITHUB_PAT_X` or `GITHUB_PAT_Y` for a matching `codex.github-identity`.
3. `GH_TOKEN`.
4. `GITHUB_TOKEN`.

`radar validate` with no paths is the daily fail-closed health gate. It runs cache
retention, requires both canonical queue and release-delta snapshots, and rejects
either snapshot after 12 hours. Its bounded JSON report includes the cache-GC result.
`radar validate --bootstrap` is the only mode that accepts a first-run cache, and it
accepts only a completely empty generated cache. A partial directory tree, ledger,
temporary file, or other generated entry fails with `RADAR_BOOTSTRAP_NONEMPTY`. An
explicit validation path with `--bootstrap` fails with `RADAR_BOOTSTRAP_SCOPE`.
An explicit missing path is always an error. Use `--max-age-hours <HOURS>` to set a
machine-enforced limit for explicit paths.

`radar cache-gc` enforces the owner-only local retention policy. Bundles, review
queues, reviews, impacts, Control Plane candidates, signals, release deltas, and
generated artifacts have 30-day, 256-file, and 64 MiB per-collection limits. Ledger
tables have 30-day and 10,000-row limits, and the ledger has a 64 MiB limit.
Directories must be mode `0700`; JSON and SQLite files must be mode `0600`.
Radar resolves the cache from a trusted repository-relative root and uses
descriptor-relative, no-follow traversal for cache reads and mutations. Symbolic-link
ancestors, `..`, wrong owners, wrong modes, unexpected hard links, and entry
replacement fail closed. One process lock serializes every Radar cache writer with
cache GC. GC revalidates each scanned file identity before deletion and removes
abandoned `.radar-tmp-*` files while it holds that lock. Atomic writes use a fresh
128-bit operating-system random nonce and reserve the lock and temporary-file names.

Ledger fields and tables are bounded when written. Radar reads the bounded SQLite
image through the fixed cache descriptor, operates on it in memory, and atomically
persists it through the same descriptor. Retention removes the oldest rows first and
keeps the ledger lock through open, pruning, compaction, the final size decision,
atomic replacement, and directory synchronization. Radar never silently deletes or
resets an oversized ledger. If pruning cannot meet the byte limit, it preserves the
ledger and fails with `RADAR_LEDGER_OVERSIZE`.

One queue subject becomes eligible for content consideration only through the bounded
cross-artifact gate:

```sh
radar content-eligibility \
  --review .agent/automations/radar/cache/github/reviews/<REVIEW>.json \
  --impact .agent/automations/radar/cache/github/impact/<IMPACT>.json
```

The command accepts exactly one `upstream_review/v1` and one
`upstream_impact/v1`. The review subject must carry the exact normalized queue commit
set and upstream head. The impact must bind the review artifact SHA-256, review
identity, upstream head, and commit set. URL reuse, slug reuse, or freshness cannot
substitute for exact lineage. The artifacts must also carry an explicit publish
decision and content angle. All three inputs must share one private Radar cache root
or all be external. A successful command emits a
`radar_content_eligibility/v1` receipt with the queue, review, and impact SHA-256
values, the normalized commit set, upstream head, and a canonical
`lineage_sha256`.

The daily Content Manager can select at most one current queue subject for a
bounded source-reading pass without a model or network call:

```sh
radar review-next
```

`review-next` reads only the canonical owner-only queue cache. Under one cache
lock, it validates freshness and deterministically selects the first critical,
high, or normal merged or commit-only subject with meaningful surface and
attention evidence. Metadata is triage evidence only. The command never writes
`upstream_review/v1` or `upstream_impact/v1`, never calls `content-eligibility`,
and cannot make a subject publish eligible.

The bounded `needs_source_review` result contains the exact selected identity,
title, source state, normalized commit set, source reference, and an immutable
queue-generation identity: the cache-relative queue reference, exact queue
SHA-256, `generated_at`, and upstream head. Its `selection_sha256` binds that
complete receipt. The native Content Manager must use the receipt for one
source-reading pass and write source-backed review and impact artifacts through
the existing artifact contracts. `content-eligibility` remains the only command
that can prove those artifacts publish eligible. An empty or ineligible queue
returns `no_eligible_item` bound to the queue generation.

Queue and release-delta refresh commands always report
`material_changed`, `written`, and `refreshed_at`. A successful freshness-only
refresh rewrites `generated_at` and reports `material_changed = false` with
`written = true`. Comparison and replacement use one lock scope. A refresh with an
older `generated_at` cannot replace a newer observation.

Radar ledger schema 6 is a clean-start cache contract. Radar rejects older local
ledger schemas instead of migrating them. First initialization is one
`BEGIN IMMEDIATE` transaction through schema inventory validation and commit, so an
interrupted initialization can be restarted without accepting a partial schema.
