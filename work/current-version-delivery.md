# Chief workspace delivery

## Goal

Deliver a conversation-first workspace for general multi-agent work. Chief owns
context, coordination, and assessment. The model chooses its method. Codex owns
native conversations and execution. Decodex owns presentation and durable work
relationships. Development is the first acceptance case, not a domain restriction.

## Acceptance ledger

- [x] General Chief responsibilities, with proportionate evidence and review.
- [x] Native model catalog, reasoning options, and Fast capability in the composer.
- [x] Verify Memory configuration in the actual runtime; do not change user settings.
- [ ] Reliable conversation, steering, interruption, and restart acceptance.
- [x] Worker evidence inspection and usable dependency/organization views.
- [x] Real development and non-development collaboration acceptance.
- [ ] Native subscription dictation feasibility and integration if supported.
- [ ] Final native visual, keyboard, and interaction acceptance.

Existing implementations must be checked before claiming completion. Do not replace
real provider evidence with fixtures. Keep original work and user drafts. Do not
install over the user's application or run two preview instances.

## Native compatibility baseline

Installed: codex-cli 0.154.0-alpha.6.2. Official source reference:
8452164c761c9225b2ee12c2bd1d48f818573704. Installed experimental JSON schemas
are under target/codex-voice-schema. Read model/list and config/read without
starting a provider turn. Runtime metadata must come from its owned connection.

## Implemented in this pass

- General-purpose Chief instructions replace the code-review-specific policy.
- Native `model/list` supplies model IDs, names, reasoning choices, image input,
  and Fast support. The composer preserves an explicit selection when offline.
  No invented catalog is shown before a Chief connection exists.
- Native `experimentalFeature/list` supplies the observed Memory flag. Settings
  displays this read-only evidence; this does not establish Memory v2 selection.
- The retained account bridge admits the exact read-only feature-list RPC. Feature
  writes, config writes, and memory reset remain outside this capability.
- Worker activity opens exact native command output, text tool results, or diffs.
  Source thread/turn/item identities are checked. Reasoning is not exposed.
- Tree and Graph show unresolved dependency blocking. Graph hover names blockers.
- Interrupted/failed turns without an answer show an execution notice instead of
  a fabricated assistant response.
- A user turn carries previously delivered, undisposed evidence into its tool
  context. The user message remains plain text. Fresh evidence keeps its own wake;
  no worker execution is replayed.

## Verification so far

- Protocol and runtime unit suites passed; GPUI: 165 passed, 5 ignored.
- New user-turn carryover regression: three inbox tests passed.
- Strict all-target, all-feature Clippy passed for GPUI, runtime, and protocol.
- Sixteen vNext architecture checks passed. Protocol is now 2.23.
- Real provider engineering and research tasks passed. Chief assessed both results;
  native worker output was readable through the public service query.
  Report: target/chief-live-acceptance.md.
- Live runtime returned five models and Memory enabled.
- Nested manager results and partial output passed. A separate injected-disconnect
  check could not prove group death because a native Computer Use helper remained
  in that process group. The service correctly retained its recovery fence. Do not
  terminate a potentially shared helper or claim this edge case is solved.

## Remaining acceptance

The complete post-fix repair/restart run and native window inspection are recorded
below when finished. Voice remains unavailable for the native-subscription reasons
in voice-input.md. No separate API, microphone capture, external memory service,
commit, push, or production installation was performed.

## Final service results

The post-fix full live run exited successfully. It verified two independent worker
results, same-account timer reconnection after an owned-process disconnect,
continuation of the original worker for repair, stable-source event deduplication,
pending obligations through service restart, preservation of every original thread,
a later Chief user turn, and bounded readable long output. The carryover check also
passed; its old harness could observe a pre-acknowledgment claim as the first receipt.
The harness now requires a non-empty acknowledged turn ID. The deterministic regression
already checks a completed previous delivery and exact unchanged user text.

Final runtime unit suite: 317 passed, 1 ignored. GPUI suite: 165 passed, 5 ignored;
31 relevant Chief tests passed again after the final palette size adjustment.
Protocol suite: 74 passed. Strict Clippy and all sixteen architecture checks passed.

The production preview history reopened without changing work. Its bound account
currently has 100% weekly usage, so its native connection and catalog remain unavailable.
The app now distinguishes this observed quota exhaustion from missing quota evidence.
No automatic account rebinding or quota reset was performed. Live qualification used
a separately enrolled available account in a disposable profile.

All sixteen task-owned qualification threads were archived after their service runs
ended. Original user projects and conversation history were retained.

## Preview delivery

The final signed preview is target/chief-workspace-app/Decodex.app. Only the exact
idle task-owned preview was restarted. The installed /Applications app was not
modified. Native accessibility readback confirmed the retained user history,
programmer-mode send shortcut, and panel controls. The model popup correctly
reported unavailable while the original bound account was depleted. Native screenshot
capture intermittently returned white frames; final native animation acceptance is
not established. The editor layout was also inspected through the GPUI capture at
target/visual-tests/final-editor.png (a visual fixture, not a real task report).

## Incremental completion (2026-09-17)

Chief account rotation is implemented and verified against the original live root;
see chief-account-rotation.md. Superseded credential recovery no longer falsely
blocks process admission. Earlier account-affinity limitations above describe the
previous delivery. Composer menus now retain their content while animating closed.
Full native animation acceptance and the surviving-helper recovery boundary remain
open. Neither dictation nor Live has been integrated into the GPUI app yet.


## Bounded cooperative process shutdown (2026-09-17)

The retained bridge now closes child stdin before it waits to deliver the terminal
event. A full event queue can no longer keep that private lifetime channel open.
Exact owned termination closes private channels first and gives Codex up to half
the existing termination budget (at most two seconds) to dispose its threads and
helpers. TERM and KILL escalation remain within the original total budget.

Evidence: the blocked-terminal regression uses a real Unix socket and verifies EOF
before the consumer drains its full queue. Runtime tests: 319 passed, 1 ignored.
Strict all-target/all-feature runtime Clippy passed. The original orphan helper PID
62489 was absent on reinspection. No unrelated helper was signaled. This prevents
one premature-termination path; it does not authorize killing restored or shared
helpers. A surviving group still requires positive quiescence before reuse.
