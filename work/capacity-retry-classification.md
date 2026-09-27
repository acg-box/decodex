# Current native capacity and quota qualification

## Qualification on 2026-09-26

Restore the full inherited native capacity fixture and all its assertions. Adapt
only the raw-frame backend call to the current shared Responses fixture. Keep
one HTTP implementation; successful fixed-usage and variable-usage responses
retain their existing frame construction.

The installed Codex CLI 0.158.0-alpha.2 passes both explicit native tests:

- A confirmed overload schedules capacity recovery. After coordinator, native
  process and store restart, the retry uses the selected `gpt-5.6-terra` model
  and `medium` effort. The original input occurs once. A later effort-only choice
  uses `low` effort and preserves the selected model.
- `slow_down` terminates as `rateLimitExceeded` after its configured native
  stream retry. Credit, organization and project quota errors terminate as
  `usageLimitExceeded`. No case creates a local capacity retry, including after
  database reopen. Observed request counts are two, one, one and one.

The current database owner requires exact failed-turn `serverOverloaded` evidence
and eligible native readback. It limits recovery to three attempts. The runtime
binds the saved model and effort and cancels a retry when the native selection
changes. Eleven existing capacity tests pass, including revert, changed selection,
refused continuation and recovery without replay. Existing compaction and history
native fixtures pass through the factored backend. Strict runtime Clippy passes
with all features and targets.

Read the fixed upstream SSE classifiers and retry-delay parser at cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Native owns raw error classification
and stream retry. No production retry policy, account, model fallback, normal
profile or automation changes in this batch. These local synthetic tests do not
qualify HTTP 503 timing, real account errors or signed desktop interaction.

The complete fixture and this document have current dispositions. The shared
native-test parent and broader recovery acceptance remain open.

## Historical review

The following preserved review describes earlier evidence. Its log references
and unmerged status are historical; the current qualification above supersedes
those status statements.

# Native throttling and quota classification

Upstream `31ffe2bc9adccfe5fd3d29208250f796a13aa7a0` distinguishes transient
throttling from model capacity and exhausted budgets. The full patch and final
classification delta were reviewed through cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

| Native source error | App-server classification | Retry owner |
| --- | --- | --- |
| `server_is_overloaded` | `serverOverloaded` | Decodex bounded capacity recovery after exact terminal evidence |
| `slow_down` | `rateLimitExceeded` | Native Codex stream recovery |
| `credit_balance_exhausted` | `usageLimitExceeded` | No automatic capacity retry |
| `organization_spend_limit_exceeded` | `usageLimitExceeded` | No automatic capacity retry |
| `project_spend_limit_exceeded` | `usageLimitExceeded` | No automatic capacity retry |

The change also classifies HTTP 503 `slow_down` as a rate limit and parses retry
delays from SSE error messages. Local code already keys capacity recovery on the
exact `serverOverloaded` classification. It does not reinterpret raw error text
or select LunaReserve.

The added installed-native regression runs all four throttling/quota SSE cases
through Chief and its store. With one native stream retry configured, `slow_down`
makes two requests and terminates as `rateLimitExceeded`. Each quota case makes
one request and terminates as `usageLimitExceeded`. No case schedules a Decodex
capacity retry, including after database reopen. Test and strict runtime lint pass:
`/tmp/decodex-1310-native.log`, `/tmp/decodex-1310-lint.log`.

This test uses isolated loopback Responses fixtures. It does not measure the HTTP
503 retry delay or prove production account error handling. Existing model-capacity
restart/same-model recovery tests remain separate. The new regression is unmerged.
