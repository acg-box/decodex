# Radar Automation Operations

This directory owns reusable repo-local source for the Radar auxiliary evidence tool.

- `radar.toml`: canonical Radar cache and handoff path contract.
- `scripts/github/`: bounded GitHub and Codex analysis helper contracts.
- `skills/`: repo-local Radar skills for upstream triage, code analysis, release
  analysis, and signal drafting.

The obsolete Radar schedule and prompts were removed. The current upstream adaptation
loop does not depend on them.

Generated Radar state belongs under `.agent/automations/radar/cache`.
The current Radar ledger is a clean-start cache. No historical schema migration is
supported. First-run `radar validate --bootstrap` accepts only a completely empty
generated cache; any partial tree, ledger, or temporary file fails closed. Combining
`--bootstrap` with explicit validation paths fails with `RADAR_BOOTSTRAP_SCOPE`.

Radar owns upstream evidence, `upstream_review/v1`, `upstream_impact/v1`,
`analysis_draft`, `signal_entry/v1`, `release_delta/v1`, and
`control_plane_upgrade_candidate/v1` artifacts. It does not own Decodex runtime
commands or Decodex social publication artifacts.

Decodex Publisher consumes Radar handoff evidence and owns
`social_candidate/v1`, `social_publish_reservation/v1`, `social_post/v1`, and
`social_outcome/v1` under `.agent/automations/decodex/cache/social`. Content Manager
owns local `social_strategy/v1` records. Social and strategy records are not Radar
cache inputs and must never be committed or uploaded to GitHub.

The current upstream adaptation tasks live under `automations/upstream/`. Content
Manager invokes Radar as an evidence tool. The default sync path does not generate a
separate live Radar job from this directory.

GitHub-backed Radar commands prefer the repository-routed identity and then `GH_TOKEN`
and `GITHUB_TOKEN`. An explicit `--token-env` is fail-closed when the named variable is
missing or empty.

Default cache validation runs local retention, requires canonical review-queue and
release-delta snapshots, applies a 12-hour machine freshness limit, and includes a
bounded cache-GC report. A first-run empty cache must use the explicit
`radar validate --bootstrap` mode. Explicit missing validation paths remain errors.
The one-subject handoff gate applies the same freshness limit to its selected upstream
review and impact.

`radar cache-gc` owns deterministic local retention: 30 days, 256 files, and 64 MiB
for each bundle, review-queue, committed content-review pair, content-review staging,
Control Plane candidate, signal, release-delta, and generated collection; 30 days,
10,000 rows per table, and 64 MiB
for the disposable ledger. Cache directories are `0700`; JSON and SQLite files are
`0600`. Descriptor-relative no-follow traversal rejects
symbolic-link ancestors, `..`, wrong ownership or mode, unexpected hard links, and
path replacement. One cache-wide process lock serializes every writer and GC. GC
removes abandoned internal temporary files and directories under that lock. It
removes a committed review pair as one directory unit. Internal lock and temporary
names cannot be output destinations.
Ledger writes enforce bounded fields and rows, prune oldest-first, and never reset an
oversized ledger. An irreducible oversized ledger fails with
`RADAR_LEDGER_OVERSIZE`. Radar loads the bounded SQLite image through the fixed cache
descriptor, operates on it in memory, and atomically replaces it through that
descriptor while the cache lock remains held.

`radar content-pair-commit` is the only authoritative review-pair writer. It accepts
one create-only mode-`0600` `radar_content_review_pair_staging/v1` file from the fixed
staging root. The staged impact must use exactly 64 zeroes for
`review_lineage.artifact_sha256`; this required sentinel is not authoritative. The
caller does not serialize or hash the review. Radar serializes the final review,
inserts its exact byte SHA-256 into the impact, validates current queue lineage, and
atomically commits both artifacts in one run-owned directory. Exact retry recovers.
Conflicting retry and duplicate subject fail closed. Staging is removed only after the
installed pair is read back and confirmed.

`radar content-eligibility` is the one-subject handoff gate. It requires one current
queue subject and one committed pair with a matching `upstream_review/v1` and
`upstream_impact/v1` before it reports that the subject is eligible for downstream
content consideration. Eligibility requires exact normalized commit sets and
upstream head plus an impact binding to the review SHA-256 and review identity. It
rejects mixed private-cache and external input sets. Its
`radar_content_eligibility/v1` receipt includes exact queue, review, and impact
SHA-256 values, the normalized commit set, upstream head, and canonical lineage
SHA-256. It does not create social artifacts or publish content.

`radar review-next` requires `--expected-queue-sha256` from the preceding successful
queue refresh report. It compares that receipt with the currently locked queue bytes
before it validates committed handled state or selects a subject. It deterministically
skips repository, subject, and normalized commit-set identities that already have one
valid pair. Queue upstream-head changes do not repeat a handled review; commit-set
changes do. The report binds the handled count and handled-state SHA-256. Malformed,
duplicate, ambiguous, or refresh-mismatched state fails closed.

Every queue or release-delta refresh reports whether material content changed,
whether the artifact was written, and the successful refresh time. A
queue refresh also reports the exact canonical queue-byte SHA-256. A freshness-only
write remains observable. One lock covers comparison and replacement, and an older
observation cannot overwrite a newer one.
