# Decodex Publisher

`decodex-publisher` is the auxiliary publishing handoff tool for Decodex-owned
social artifacts. Radar produces upstream evidence and `signal_entry/v1`
artifacts; this tool owns `social_candidate/v1`, `social_publish_reservation/v1`,
and `social_post/v1` validation and reservation workflows.

```sh
decodex-publisher social reserve-publish \
  --slug openai-codex-pr-22414 \
  --mode operator_impact \
  --idempotency-key x:decodexspace:operator_impact:openai-codex-pr-22414 \
  --reserved-at 2026-06-02T03:00:00Z \
  --expires-at 2026-06-02T03:15:00Z \
  --day 2026-06-02 \
  --candidate .agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json \
  --duplicate-key openai-codex-pr-22414

decodex-publisher validate-social .agent/automations/decodex/cache/social/x
```
