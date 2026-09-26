# Model and routing acceptance

Classification: core preservation of existing consumers. This audit adds qualification
of existing routing and recovery. It does not add a profile-switch control or another
draft store. The upstream cutoff remains
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Existing model and draft owners

The merged model, defaults and ordinary recovery deliveries are listed in
[the adoption register](upstream-adoption-review.md). Current source retains native
model and effort inheritance, explicit execution overrides, exact command receipts
and source-bound cold recovery. The combined affected-package nextest run at the
PR1518 application source passed 2,377 tests, with 68 skipped. Its one leaky-handle
warning was on an unchanged pure account-profile test; an isolated run passed.

`ordinary_drafts.rs::sync_ordinary_drafts` uses the existing Chief draft writer before
service setup. `chief_ordinary_storage.rs` selects `unbound_ordinary` without a
profile. The first configured profile adopts that content without granting submission
authority. Fresh rendered tests cover unsaved quit gating, real-store cold reopen and
first-profile adoption. The old pre-profile implementation gap is resolved.

`Conversations::production` reads its working directory once from the supported
environment override or HOME. `ConversationsInner` has no directory setter. The main
app constructs its Chief profile and installs lifecycle controllers once. There is no
current ordinary in-session directory/service-switch control to implement or qualify.
This disposition does not apply to Chief creation-directory editing, existing ordinary
routing controls or cold startup under a different profile.

## Account capacity and rotation

`AccountService::select_chief_route` prefers the current eligible account, checks quota
and credential readiness, and excludes occupied account process slots. It can select
another available account after exhaustion. Existing selection tests cover exhausted,
disabled, credential-absent and unknown-quota candidates.

`ChiefHost::rotate_exhausted` pauses dispatch on exhaustion. Active voice or non-idle
work prevents rotation. It must positively close the old process before restoration.
The database independently rejects account changes while a prior process is not dead,
work is uncertain, or a prompt-edit receipt remains unresolved. It preserves the
original thread across a permitted change and database reopen.

Installed Codex 0.158.0-alpha.2 passed the complete public-socket qualification with two
synthetic accounts and a local Responses provider. After the first account exhausted
both quota windows, the host admitted a new process generation on the second account.
It restored the same native thread without another model request. One subsequent
explicit send produced exactly one request, and the new native history contained that
message. No real account credentials, provider service or production database was used.

Run the existing isolated native socket fixture with
`DECODEX_TEST_ACCOUNT_ROTATION=1` to include this branch. The test requires its private
HOME and fixture marker, as documented in [task recaps](task-recaps.md). It can run with
`DECODEX_TEST_PROMPT_REVERT=1` to qualify rotation after the complete editing lifecycle.

## Unknown root creation

A failed native thread-start response is not proof that no thread exists. The fixed
upstream `ThreadStartParams` and installed generated schema expose no caller-supplied
thread identity or idempotency key that would make an automatic retry safe.

The existing root reservation stores the original input before process admission.
The strengthened `failed_thread_start_remains_unknown_without_retry` regression
reopens the real database and supplies a fresh working native connection. Recovery
retains the original input and Unknown state with no thread ID. Continuing the reserved
root is refused, creating a replacement root is refused, and no native request is sent.
This is a conservative supported outcome, not a claim that the unknown native thread
was found or that creation succeeded. Do not infer identity from the newest thread.

## Evidence boundary

R03's remaining implementation questions have source dispositions and targeted
qualification. PR1519 merged these additions as
`8407e7a102f8df4385ff3c4ffbdbedf4caf823a7`; R03 is closed. Signed whole-app quit, menu, Dock, relaunch, export and blank-task acceptance remain
in R07/R12. Live voice and remaining recap acceptance stay in R06. No result in this
record establishes public release, installation or complete upstream catch-up.
