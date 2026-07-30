# Decodex Automation Skills

These skills define the content-quality and bounded-publication policy for the two
Decodex content automations.

- `x-post-quality-system`: decide whether a source-backed candidate is useful
  enough to publish.
- `x-post-publisher`: reserve, publish, verify, observe, and terminalize through
  the checked-in `decodex-publisher` auxiliary.
- `references/social-release-publisher-gates.md`: source, lineage, quality, and
  cost gates.
- `references/scheduled-run-thread-retention.md`: successful-task cleanup and
  failed-task visibility.

`decodex-publisher` is the only X endpoint client. It uses xurl with a fixed target
account, one-post-per-day limit, no-URL public text policy, and monthly cost cap.
Generated records are private local state under `.agent/automations/decodex/cache`.
Run `decodex-publisher validate-social` before and after each terminal operation.
