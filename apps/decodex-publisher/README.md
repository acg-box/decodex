# Decodex Publisher

`decodex-publisher` is the auxiliary publishing handoff tool for Decodex-owned
social artifacts. Radar produces upstream evidence and `signal_entry/v1`
artifacts; this tool owns `social_candidate/v1`, `social_publish_reservation/v1`,
`social_post/v1`, `social_outcome/v1`, and `social_strategy/v1` validation, browser
lease, and reservation workflows. X publication is browser-only. The tool does not
call X MCP or X API. Generated social, strategy, browser-session, and lease records
are local-only under `.agent/`. Do not commit or archive them to Git.

```sh
decodex-publisher social acquire-browser-lease
# Keep the returned lease token only in the current process context.

decodex-publisher social reserve-publish \
  --slug openai-codex-pr-22414 \
  --mode operator_impact \
  --idempotency-key x:decodexspace:operator_impact:openai-codex-pr-22414 \
  --reserved-at 2026-06-02T03:00:00Z \
  --expires-at 2026-06-02T03:15:00Z \
  --day 2026-06-02 \
  --candidate .agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json \
  --duplicate-key openai-codex-pr-22414 \
  --browser-lease-token "<lease-token>"

decodex-publisher social renew-browser-lease --lease-token "<lease-token>"
decodex-publisher social verify-browser-lease --lease-token "<lease-token>"

decodex-publisher validate-social

decodex-publisher social release-browser-lease --lease-token "<lease-token>"
```
