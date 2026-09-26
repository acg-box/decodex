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

## Signed desktop inspection on 2026-09-26

A normal signed application built from
`048768e2e6b0664753d5cc2deddcf209836d873c`, with a clean build identity and protocol
2.90, passed bundle, embedded-component and signature verification. The isolated
same-UID service contained one ordinary task that required model review.

The running application displayed Main and Projects. The inspected File menu and
General settings did not expose History. The source confirms that normal startup
selects Chief and that Command-1 and Command-2 both select Chief. The
`ActivateConversations` action still has a declaration, handler and registration,
but the source search found no production menu or key binding that dispatches it.
The History renderer and its programmatic UI tests remain in the source.

The inspection did not reach the ordinary task review controls. The desktop quit
normally. After service shutdown, readback retained the review flag and zero turns.
The interactive fixture failed its required one-submission assertion with zero
provider requests. This is failed desktop acceptance, not a successful no-replay
qualification. The diagnostic patch, output and readback were preserved outside
application sources; the isolated home was removed after process and open-file
checks. No navigation control was added to make the test pass.

Treat the ordinary History workbench as a separate optional, currently unexposed
product surface in the final removal review. Its restored protocol and storage
contracts do not establish that it is part of the current Chief user journey.
The user can assess the complete surface and its consumers before removal. R03
and shared desktop acceptance remain open; native fixture success does not close
this user-interface gap.
