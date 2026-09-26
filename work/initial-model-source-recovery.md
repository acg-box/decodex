# Initial model source recovery

## Problem

The inherited initial model review flow retained the account identity and revision
that supplied an ordinary task's model choices. The current request store omitted
that observation. A later account selection could therefore use choices from a
different account without requiring review.

## Storage and routing foundation

Schema 46 extends the existing `quick_task_requests` owner with an optional paired
account identity and positive revision, plus a review-required flag. Existing
requests retain their prompt, directory, model, nullable reasoning effort and
nullable service tier. Their source remains absent.

Creation stores and reads this source. Replaying the same creation command with a
changed or absent source returns an idempotency conflict. A routing successor
retains the source. Initial turn admission requires the stored source, when
present, to match the session account and revision.

Initial routing compares the selected account and revision before it writes a
routing decision. A mismatch stores the review-required flag and updates the task
observation time without creating a route, session or turn. Later retries still
require review, even if the account revision now matches.

Explicit review uses the existing transaction and command receipt owners. It
requires an active task at the displayed revision, the review-required flag, and
no existing routing decision, session or turn. It changes only the model choices
and source, clears the flag, and advances the conversation revision. It preserves
the prompt and directory. Nullable effort and service tier remain nullable.
Concurrent confirmations have one winner; replay returns its saved revision.
Confirmation alone does not spawn or send.

## Delivery boundary

The public creation command now carries the discovery source. Explicit creation
without a catalog observation remains available. Source-bearing creation and
receipt queries share a source-bound fingerprint; source-less requests retain
their previous fingerprint.

Protocol 2.90 adds the review query, confirmation command, and review-required
state. The service reads the original request and queries the native catalog at
its saved directory. It checks the task revision before and after discovery.
The desktop presents the saved request and requires an explicit confirmation.
A stale selection cannot apply discovery, and a lost confirmation reply triggers
readback without automatic replay. The current nullable reasoning effort and
service tier contracts remain in effect.

The database foundation is PR1537. The protocol, service and desktop integration
are a subsequent batch. R03 remains open until that batch is merged and its
remaining acceptance is recorded. Shared signed application lifecycle acceptance
remains in R07/R12.

The fixed upstream cutoff remains
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. This change restores a Decodex account
observation contract; it does not replace native model catalog or execution
settings authority.

## Validation

The migration fixture covers old rows, nullable execution settings, invalid source
pairs and revisions, review flag constraints, and repeated migration.
The restart integration fixtures cover source persistence, changed creation
replays, account and revision routing mismatches, persistent review after the
source matches again, successor inheritance, concurrent and stale confirmation,
cold receipt replay, preserved input, and rejection after routing.

The installed Codex 0.158.0-alpha.2 fixtures use synthetic credentials, a loopback
provider, and isolated temporary homes. They verify saved-directory defaults
without inference, explicit confirmation with one provider request despite
retries, cold readback, and retained project warnings. Successful fixture homes
are removed after shutdown. These fixtures do not prove real-provider behavior
or final signed desktop presentation.

Final signed desktop acceptance remains open. Maintenance automation remains
paused.
