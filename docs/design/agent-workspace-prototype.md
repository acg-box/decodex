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
