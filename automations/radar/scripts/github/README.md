# Radar GitHub Helpers

Use `radar --help` for collection, validation, release deltas, and backfill.
Use `python3 automations/radar/scripts/github/run_codex_analysis.py --help` for
explicitly authorized standalone AI analysis. The helper requires an AI-boundary
opt-in and a bundle export inside the repository, outside the private Radar cache.
It prints validated analysis JSON to stdout. The Rust backfill command owns its
temporary export and cleanup; the Python helper does not impose an output byte limit.

See the [Radar contracts](../../../../openwiki/integrations/radar-publisher-contracts.md)
for responsibilities and the [repo-local skills](../../skills/README.md) for analysis
instructions. Helpers and schemas do not authorize code changes or publication.
