# Chief composer

## Interaction

The user-supplied ChatGPT screenshot is the primary layout reference. Keep the
shared glass material. Use a compact, growing text area above one control row.
The left control adds files. The right controls show context percentage, Fast,
model and reasoning selection, and Send. Open model and reasoning choices in a
small floating panel above the input. Escape or a selection closes the panel.

Enter sends. Shift-Enter inserts a visible newline. Text wraps to the available
width and grows to seven visible lines. The caret follows edits and arrow keys.
The context percentage is absent before a nonzero report and known capacity.
Hover shows only the context count and capacity. Per-turn input/output totals remain with the reply.

Select files with the native picker, drop files, or paste images/files. Image
chips include thumbnails; remove a chip before sending to omit that attachment.
Clipboard images are stored with owner-only permissions in ~/.decodex/attachments.
Other attachments reference the original local file; the file must remain
available until the provider reads it. Files are not uploaded to a separate
storage service. Up to 16 files can accompany one message.

## Delivery

Protocol 2.20 adds configured Chief start/send operations. Existing plain-text
operations remain supported. The host saves text, settings, and attachments in
one immutable inbox event before provider effects. Each turn uses that event's
model, reasoning effort, and Fast selection. UI changes after Send do not modify
an accepted message. Attachment drafts follow manager navigation and are cleared
only after acceptance; rejected or uncertain sends retain them.

Codex native `turn/start` receives `model`, `effort`, and `serviceTier` (`priority`
or explicit null). Images use `localImage`; other files use a user-selected path
reference in text input. Attachment references are included in saved history.
No synthetic production messages are created for UI verification.

## Upstream evidence

- Official Codex reference: fd346b8dbaa24573a0244bc917811849d27c4cf4,
  app-server-protocol/src/protocol/v2/turn.rs.
- Installed CLI schema generated at target/codex-composer-schema verifies model,
  effort, serviceTier, and localImage support.
- The correct Pi Desktop repository is https://github.com/vastsa/pi-desktop.
  It is not the primary visual reference for this implementation.

## Verification

Native GPUI tests check Shift-Enter, Unicode line geometry, wrapping, bounded
height, and caret positioning. Runtime tests check that configured queued input
sends once with the exact captured settings, native image, and file reference;
turning Fast off explicitly clears the prior tier. Existing UI and protocol
regression suites also run. Visual fixtures live only in the capture build.

Final checks: GPUI suites passed 157 and 155 tests (4 and 2 existing ignored),
Chief runtime passed 55 tests, and protocol passed 74 tests. Strict Clippy passed.
The signed native preview was opened as a single instance. Native controls
confirmed a two-line Chinese draft after Shift-Enter, model-menu selection, and
adding/removing the user-provided image through the file picker. Verification
drafts were cleared without sending a production message. Native screenshots
worked at launch but later returned the known white-capture artifact; use the
rendered fixture for multiline/menu geometry, not as evidence of desktop blur.

## Empty-state prompts

The placeholder uses 12 curated, API-sourced English quotes with author attribution.
Choose a new phrase at launch, manager navigation, and successful message
acceptance. Never change it during editing or on a render timer. Consecutive
choices differ. ZenQuotes supplies short English quotes in the background, with a six-second
timeout and a 128 KiB response bound. Request an online batch at startup, manager navigation, and accepted sends,
with a one-minute request throttle. Save successful batches in
~/Library/Caches/Decodex/quotes.json. Use a stale cache or curated offline quotes
when offline. The author follows the quote in the placeholder. The required ZenQuotes source link appears on hover
over the empty input. Requests contain no conversation or account data.
The context indicator is a static usage ring; hover shows percentage and counts.

## Programmer mode and live delivery

The Chief composer starts in programmer mode. The `</>` control switches between
programmer and ordinary text entry. Programmer mode uses Menlo, logical line
numbers, and up to 12 visible rows before internal scrolling. Wrapped rows keep
one logical line number. The composer has 16 px of bottom separation.

- Enter inserts a newline; Command-Enter sends.
- Command-D selects the current word, then adds the next matching selection.
- Option-Up/Down adds a cursor on the adjacent logical line.
- Tab inserts four spaces. Escape removes secondary cursors.
- Command-Z and Command-Shift-Z undo and redo complete editing transactions.
- Control-C interrupts the exact observed running turn. Command-C still copies.

The delivery control switches Steer and Queue, with Steer selected initially.
When idle, both start a new turn. During a running turn, Steer uses native
`turn/steer` with `expectedTurnId` and `clientUserMessageId`; Queue keeps the
existing durable inbox path for the next turn. Steering includes attachments but
does not change the current model, reasoning effort, or service tier.
A nonempty draft shows Send; an empty composer shows Stop while running.

Protocol 2.21 adds the explicit Steer operation. A durable attempt precedes the
provider call. It cannot wake a new turn or replay after restart. Confirmed
acceptance writes a user-message receipt tied to that exact turn. Rejection keeps
the draft. Unknown acceptance keeps the draft and requires inspection. A stale
turn does not silently fall back to Queue. The event source remains immutable.

The installed 0.154.0-alpha.6.2 schema includes TurnSteerParams and TurnInterruptParams.
The official reference commit above includes active-turn, stale-turn, and
client-message identity tests in app-server/tests/suite/v2/turn_steer.rs.

Validation for this iteration: the GPUI suites passed 163 and 161 tests; the
Chief runtime suite passed 57 tests, followed by three focused steering tests
for the final receipt change. The delivery-mode and stop-state test passed.
Protocol passed 74 tests. Strict Clippy, formatting, whitespace checks, and the
bundle protocol boundary check passed. The signed preview was opened as one
instance. The native screenshot confirmed the composer layout and restored real
history. Further live keyboard checks stopped when the user changed the window;
keyboard behavior is covered by the GPUI tests. No production message was sent.

## Composer control design — September 16

The composer uses a compact violet send key with a custom diagonal launch mark. An empty draft dims this key. The existing stop state uses a warm neutral fill and a square mark. Keyboard submission and interruption keep their existing behavior.

Model selection uses a two-column palette. Model names and versions have separate visual levels. The toolbar shows the short family name; the tooltip and palette show the full model. Reasoning uses discrete labeled levels with a six-bar indicator. The current model's existing capability table controls the available levels. Switching to a model with fewer reasoning levels clamps an unsupported level to its highest supported value. Fast remains independent.

Controls retain keyboard activation, hover and press motion, and the existing animated popover. The glass composer material remains unchanged. Visual fixtures: `composer-menu` and `composer-effort`. Validation: GPUI suite (165 passed, 5 ignored), strict Clippy, model-switch capability regression, and inspected offscreen captures. These fixtures do not send messages.
