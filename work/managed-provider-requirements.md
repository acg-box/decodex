# Current managed-provider classification

Core: preserve proven unsent input and forbid replay after a native managed-provider
refusal. The current Chief and ordinary conversation owners implement this through
the shared refusal types and persistence transactions. The registered migration is
now 38 in the current version-48 sequence; the historical 40/42 numbering below
must not be applied to a current database. See [database mapping](database-owner-reconciliation.md).

Native policy loading remains Codex-owned. Desktop recovery and live managed-policy
delivery have separate acceptance boundaries; this note does not mark them complete.
See [current reconciliation](native-policy-routing-reconciliation.md).

## Preserved historical record

The original note follows unchanged. Its not-delivered status, migration numbers,
probe versions and remaining-work wording describe the preserved branch only.

# Managed provider requirements

Upstream: `39d193d72d7959d798642bd3e1496bb8865033b1`.
Cutoff: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Status: adaptation in progress; not delivered.

Native app-server checks retained provider selection and definitions against
current managed requirements before input, review, compaction, queue start and
active goal updates. Ordinary user or project edits do not invalidate the retained
route. Pause, clear and interrupt remain available. Realtime routing is separate.
The policy loader and comparison remain native-owned.

Chief previously treated the explicit managed-provider refusal as an unknown
turn outcome. It now uses the existing atomic refusal path when the exact native
error code and fixed message match and no earlier external effect occurred.
The database retains unsent input, clears its delivery claim and requires a user
decision. It does not replay the input. A lost response or prior injection keeps
its unknown outcome. Capacity continuations use a separate refusal receipt and
retain the original failed-turn delivery identity.

Ordinary conversations also classify this exact refusal by response identity,
error code and message. They reuse the positive non-submission transaction, which
finishes the local input and retains it in history. The recovery text requires a
connection restart and manual resend. Incorrect response IDs, codes or messages
do not receive this classification.

Schema 42 extends the claimed-capacity-retry cancellation trigger to accept a
resolved `managedProviderChanged` refusal for the exact retry and work item.
Schema 40 only accepted `serverDraining`; the first new capacity regression exposed
that missing migration. Existing migration files remain unchanged. Unknown
reasons and receipts for a different retry cannot cancel the claim. Cancelled
continuations cannot return to pending.

Validation:

- Eight runtime refusal regressions pass: `/tmp/decodex-1212-rejection.log`.
- Wire identity, code and message checks pass for ordinary policy and shutdown
  refusals: `/tmp/decodex-1212-ordinary.log`.
- Both capacity refusal reasons preserve original delivery after database reopen
  and prevent replay: `/tmp/decodex-1212-capacity-fixed.log`.
- All 16 migration tests pass, including version 41 upgrade and exact refusal
  proof: `/tmp/decodex-1212-migrations-all.log`.
- Ordinary non-submission transaction rollback and reopen pass:
  `/tmp/decodex-1212-ordinary-persistence.log`.
- Strict runtime/database Clippy passes after schema 42:
  `/tmp/decodex-1212-post-migration-lint.log`.
- Installed-native isolated local configuration edits and invalid TOML do not
  replace a retained provider route: `/tmp/decodex-1212-native-local.log`.
  Two requests reached the original local fixture provider. This did not mutate
  or test managed policy.

Installed-native policy qualification also passes with Codex 0.155.0-alpha.16:
`/tmp/decodex-1212-native-policy-enterprise.py` and
`/tmp/decodex-1212-native-policy-recovery.log`. An isolated local backend supplies
cloud requirements to synthetic external enterprise authentication. Authentication
refreshes use distinct fixture user identities so the identity-matched policy cache
does not hide the changed bundle. No real credential or host policy is used.

The retained fixture-provider thread completes once. The second bundle requires
`openai`; `configRequirements/read` confirms that requirement and `turn/start`
returns the exact -32600 policy refusal. No additional Responses request arrives.
A third empty bundle permits a manual turn on the same retained thread. Three
bundle requests and exactly two inference requests are observed. Other backend
POSTs are excluded from the inference counter. This proves the native policy
check through external-auth reload, not the timed background cache refresh or
Decodex desktop recovery interaction.

Remaining work:

- Qualify ordinary runtime-to-desktop recovery and the combined signed desktop
  flow. Wire and database fixtures alone do not prove this acceptance.
- Complete review and PR delivery. This change is not merged.
