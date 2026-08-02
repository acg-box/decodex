# Radar GitHub Helpers

These helpers support bounded, standalone Radar research. They do not orchestrate
the exact-five Decodex automations and do not authorize code or X mutations.

## Collection

Use the Rust `radar` CLI for GitHub collection, release deltas, bundle writes,
validation, and retention. It owns token routing, origin checks, pagination,
response bounds, private paths, locking, and exact readback.

```sh
radar refresh-upstream-queue
radar refresh-release-delta
radar bundle build --help
radar validate
```

Queue entries contain deterministic triage metadata. They are research hints,
not final compatibility, editorial, or publication decisions.

## Analysis Runner

`run_codex_analysis.py` prepares a bounded prompt from one private GitHub change
bundle and invokes the configured Codex analysis command. The agent owns the
analysis judgment. The runner enforces input containment, output bounds, and the
selected repo-local analysis skill.

```sh
python3 automations/radar/scripts/github/run_codex_analysis.py \
  --bundle .agent/automations/radar/cache/github/bundles/<RUN_ID>.json \
  --mode code
```

Generated analysis is advisory until an agent verifies it against official
sources. It does not become workflow state merely because the helper wrote it.

## Content Use

Content Manager may use a Radar result as secondary editorial input. It records
the original source URL in `decodex/content-evidence/1` and classifies Radar-only
material as `radar_secondary`. At least one `official_codex` or
`landed_decodex` URL is required.

There is no Radar-to-Publisher queue, review pair, eligibility gate, or private
path handoff.

## Privacy

Generated bundles, prompts, analysis, queues, and the Radar ledger stay below
`.agent/automations/radar/cache`. Do not commit or upload them. Bounded receipts
may report hashes and counts, but they must not include patch bodies, credentials,
account data, or absolute local paths.
