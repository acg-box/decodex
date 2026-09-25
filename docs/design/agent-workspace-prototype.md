# Agent workspace prototype

## Product boundary

Main is the default user entry. All visible participants are Agents. A level is
an observed position in the parent tree, not a permission or intelligence level.
Keep existing work and thread identities. Do not migrate or duplicate saved
conversations merely to change their labels.

The tree shows responsibility and native spawn relationships. The graph shows
work dependencies and reports. Work details link to dependencies, native task
references, and resources. A completed turn is not proof of delivered work.

Native descendants remain owned by Codex. Their observation must not insert
managed work records, grant manager tools, start a turn, or resume a thread.
An explicit user message requires a fresh ancestry check and native direct-input
capability. A changed active turn rejects the message. An uncertain response must
not trigger automatic replay.

## Panel controls

The initial left and right widths and bottom height are 240 logical pixels.
General settings can change the shared default sidebar width and default dock
height. These defaults apply to new windows and explicit resets; changing a
default does not overwrite a manually resized open panel.

- Ctrl+Option+- / =: decrease / increase the selected visible panel by 24 pixels.
- Ctrl+Option+0: restore its configured default.
- Add Shift: apply to all visible panels. Hidden panels are unchanged.
- Select a panel by clicking inside it. Clicking the conversation clears that
  selection. Existing Cmd+E, Cmd+B, and Cmd+J visibility controls remain.
- Full-screen Graph uses the available area independently of dock height.

## Native evidence

Reference: official openai/codex main at
`94174e44cbc54cece45f6052328ca0c2cd7a8a2a`.
Installed binary: `codex-cli 0.155.0-alpha.9.2`.
The installed experimental JSON schema includes `thread/list` ancestor filtering,
`thread/turns/list`, and `Thread.canAcceptDirectInput`.

Relevant upstream sources:

- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/thread_data.rs`
- `codex-rs/app-server/src/request_processors/thread_input.rs`

The last source explicitly prohibits direct app-server input for multi-agent v2
spawned children. The UI must show this constraint instead of an enabled composer.
The main-branch policy and installed schema are separate evidence; neither is a
live-provider acceptance result.

## Prototype limits

Native conversation inspection is a bounded recent view. Output omission must be
visible. The long-term artifact-version and cleanup ledger is not introduced by
this prototype. Existing work evidence and native resources remain authoritative;
behavioral instructions do not establish a durable verification guarantee.

## Live output delivery

Protocol 2.45 adds `WaitForChiefOutput`. The visible managed conversation owns one
cancellable local observation connection. A query waits for a persistence signal
or a 20-second heartbeat. It does not start, resume, or steer a turn.

Native output wakes observers after persistence. The existing bounded text
projection applies to live results. The UI accepts only the selected work and
current turn. A latest-value channel coalesces bursts for up to 8 ms without an
unbounded token queue. Output is rendered as received; there is no artificial
typewriter delay. Closing the view cancels its observer. A failed observation
reconnects after a bounded delay and starts with a fresh snapshot.

The 100 ms full-history poll is removed. Work state remains reconciled at 500 ms;
saved history is read on work-state changes and every two seconds for activity
and recovery. These reads do not drive live text delivery. Provider first-token
latency and network stalls remain outside the renderer's control.

## Send and interrupt presentation

Sending and interrupting have separate pending state. Interruption targets the
observed work and turn; it does not mark message delivery as uncertain or clear a
draft. Read back the snapshot before presenting an interrupt failure because a
turn can finish before the interrupt arrives. The upstream reference above rejects
`turn/interrupt` when no active turn remains (`turn_processor.rs`). Keep using the
installed native interrupt method; do not send a replacement turn.

Only unclaimed user-message receipts imply queued input. Claimed receipts from a
finished turn must not keep the primary control in its starting state. The control
crossfades fixed-size glyphs and uses a soft warm halo for the first Escape press.
A failed background snapshot read retains the confirmed view until three
consecutive failures. An explicit unavailable result or transport disconnect
continues to invalidate the view immediately.

## macOS sleep control

General exposes `Prevent system sleep`. The host's `pmset -g` `SleepDisabled`
value is authoritative; Decodex does not persist a duplicate preference. The
control sets only `pmset -a disablesleep` and reads back the applied value.
It affects both power sources and persists after Decodex exits. It does not
change display sleep, idle timers, or keyboard backlight settings.

An existing noninteractive administrator authorization is used when available.
Otherwise macOS requests administrator authorization. Cancelling authorization
reads back the unchanged state without an error notification. Other failures go
to the existing notification center. The control is absent on other platforms.

### Archive checks and account controls

Archive reads use the shared native thread catalog. A registered subordinate manager can be
inspected without admission to the main agent's execution process. The read verifies the
persisted thread binding and catalog connection again before publishing its result. Restore
commands retain the coordinator's execution ownership checks. The native list/read contract
was checked against `openai/codex` commit `94174e44cbc54cece45f6052328ca0c2cd7a8a2a`.

The archive view fences requests by its owner and connection epoch, not the snapshot refresh
counter. An ordinary refresh must not discard a completed read and leave its request locked.

Account rows expose a leading power control, routing, Reset Cards, and logout without an overflow
menu. Clicking the summary expands or closes the profile directly below that account.
Reset Card inventories also stay under their account. Re-login appears only for authentication
failure, missing credentials, a logged-out account, or an explicit login recovery operation.
Account names and quota meters remain the primary information. Icons have accessible labels
and tooltips. Logout and Reset Card redemption retain explicit confirmation; opening a card
inventory does not consume a card.

The desktop and menu-bar quota views share boundary fixtures in
`tests/fixtures/account-quota-presentation.json`: remaining quota above 50% is healthy,
above 20% is warning, and 20% or less is critical. Each UI toolkit keeps its own rendering
adapter; SwiftUI reuses one tone function for static and animated quota values. Desktop
account metrics reuse the conversation K/M/B formatter.


The Accounts eye button reveals email addresses through the existing local account-profile
API with `include_email: true`. The default view uses aliases and does not request email.
Revealed addresses remain in memory, are tied to the account revision, and are cleared when
the user hides them. Hiding also cancels the reveal task and invalidates late results.
The existing menu-bar eye control remains unchanged. The desktop power icon retains switch
accessibility semantics, supports keyboard activation, and does not expand account details.


## Weather cards

Protocol 2.46 adds optional turn identity and weather projections to saved assistant
entries. Cards use the exact turn and weather reference, retain the saved forecast,
and include a readable forecast table in Copy response. Missing data leaves the
assistant text available. Cards appear after completed history readback.

The installed app-server can omit weather data from `webSearch.results`. A bounded
read-only compatibility adapter uses only `thread/read`'s path for the selected
thread and checks the session header and turn identity. It reads at most 16 MiB,
extracts only weather text, and caches one thread's parsed results. It does not scan
session directories or alter native records. The native path field is unstable;
replace this adapter when native structured results are available. Track this in
https://github.com/acg-box/decodex/issues/1385.

The compact card uses the existing GPUI background with a translucent tint; it is
not a separate native Liquid Glass window.

Weather cards show six hours per page. Small clickable dots select each page with
a brief transition. Wheel input over the card scrolls the conversation without changing the forecast page. Single-page forecasts have no pagination controls.


### Menu-bar account UI parity

The menu-bar account list now uses the same account interaction model as Settings > Accounts:

- A leading power icon enables or disables participation. Disabled identity and quota content is dimmed; controls remain reachable.
- A square route icon pins an account. Selecting that same icon again uses the existing balanced-selection command. No separate manual-mode selector is required.
- Selecting the identity or quota area expands that account's activity inline. Only one account is expanded. Account rows have no details icon or overflow menu.
- Reset Cards remain directly visible. Redemption and logout retain their confirmation steps. Reauthentication appears only for existing login failures or explicit recovery needs.
- Email visibility, drag reordering, compact K/M/B counts, and quota tone thresholds remain shared behavioral contracts.
- In-flight commands block conflicting input without changing unrelated glyphs, opacity, or layout. The affected icon pulses when motion is enabled.

This is presentation parity across GPUI and SwiftUI, not a second routing or account-state owner.
