# Reconcile native observations

The complete inherited `chief/observations.rs` diff has three substantive changes.
The remaining observation source is unchanged. This audit does not replace native
execution or claim that every shared coordinator path is accepted.

## Exact steering receipt during history recovery

The former inner check of the returned thread ID is now enforced by
`AppServerClient::thread_read_turn`: both metadata and legacy full-history reads
validate the exact requested thread. The coordinator still selects the exact turn.
A receipt requires a native `userMessage.clientId` and the database matches the
saved work, thread, delivered turn, pending submission key and process ownership.
It records consumption once; it does not resend input.

The former question-revision condition around receipt observation is absent.
Receipt recording now occurs before the final question-projection revision check.
These are different operations: a positive historical consumption receipt is not
authority to replace the current question projection. The projection still needs
a complete read, required item and unchanged question revision. Changed recovery
state remains pending for a fresh read. Do not describe the two guards as identical.

The receipt fixture covers exact live, cold-running and cold-idle confirmation,
wrong submission IDs, duplicate receipt delivery, durable reopen and no new
`turn/start` or `turn/steer`. The question-recovery fixture separately covers
native revert, new input and disconnect during rebuilding. These tests do not
claim exhaustive combined receipt/revert race coverage.

## Revert invalidation

A native revert removes deferred closing-resume entries for that thread and calls
the existing generation-bound capacity cancellation owner before rebuilding
questions and clearing output. It does not treat already claimed delivery as
unsent. The capacity fixtures cover wrong generation/thread, duplicate revert,
durable cancellation, claimed intent preservation and no worker-completion wake.
The deferred-resume fixture covers history/turn/state changes, archive, deletion
and revert, with no native resume after invalidation.

## Live question arrivals

The common question recorder accepts a live flag. A live item notification uses
`record_live_chief_async_questions`; cold history uses the ordinary recorder.
Existing question IDs and serialized content stay the same. Repeated live delivery
does not turn an older historical question into a new arrival. The fixture checks
one historical question, one new live question, duplicate events and no turn send.
Answer parsing and other observation methods remain unchanged.

## Evidence boundary

All fixtures use isolated local state and simulated native transport. These checks
close only this inherited observation-file row. They do not prove physical desktop
notifications, installed-native shutdown, live voice or the remaining coordinator
and source-recovery acceptance. No production code changes are made in this batch.

Fresh validation passes two asynchronous recovery tests, one three-mode steering
receipt test, 15 capacity tests and one six-case deferred-resume invalidation test.
Logs are `/tmp/decodex-observations-async.log`,
`/tmp/decodex-observations-steer.log`, `/tmp/decodex-observations-capacity.log`
and `/tmp/decodex-observations-closing.log`. The original snapshot hash matches.
