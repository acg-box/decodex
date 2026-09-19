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

## Compact floating composer

Removed programmer mode, line-number painting, secondary selections, occurrence
selection, vertical cursor creation, and their key bindings. Normal multiline
editing, IME composition, clipboard operations, and undo/redo remain. Enter sends;
Shift-Enter inserts a newline, and Command-Enter remains a send shortcut.

The model label includes reasoning depth and opens one compact palette. The
microphone and input-device disclosure share a tight group. A single circular
primary action shows Live for an empty draft, an upward arrow for text or
attachments, and Stop during an active response. No empty-draft Send is implied.
The composer retains glass material with a softer edge, rounded outline, and
separate shadow. Existing hover/press and disclosure motion remain.

Validation: GPUI tests passed (167 passed, five opt-in tests ignored), strict
Clippy passed, and empty/draft palette captures were inspected. Programmer-mode
implementation and its obsolete tests were removed rather than hidden.


## Single-row composer — September 17

The default composer is one row. Draft text grows vertically when needed; long
quote placeholders stay on one line and retain their attribution tooltip. The
window keeps its existing glass material. The composer uses a subdued smoke tint,
a thin edge, and a soft shadow.

Only the primary action has a circular surface. Fast is an icon in the toolbar;
Steer/Queue is a Message delivery row in the plus menu. The model popup uses a compact 240-point plain list and one shared reasoning strip;
only the selected supported level has a filled surface. Existing keyboard actions
and hover/press motion remain.

The plus menu contains attachments and the shared microphone selector. Forward
and back arrows navigate between pages in the same popup. The back row is left
aligned. Device selection applies to the next recording for dictation or Live.

Validation: 168 GPUI tests passed; five opt-in tests were ignored. Strict Clippy
and signed bundle staging passed. Native screenshots confirmed full model names,
visible delivery and Fast controls, and the left-aligned microphone back row.
No message was sent and no recording was started during this layout check.


## Separate model and reasoning controls

The adjacent model and effort labels have separate hit targets, with no chevron.
The model popup contains only models. The effort popup is 220 points wide and
shows Reasoning, the current level, a thin track, and supported-level stops.
The thumb and fill use the shared interruptible motion primitive. Pointer input
snaps to available levels; Left/Right and Home/End support keyboard adjustment.
Releasing the pointer inside or outside the workspace ends the drag. The next
turn uses the selected effort; the current response is not restarted.

Validation: 169 GPUI tests passed, five opt-in tests ignored, and strict Clippy
passed. The new stop-mapping test covers rounding, limits, and zero/one-level
catalogs.

Native screenshot review confirmed separate controls and the effort popup. The
additional pointer-release regression passed. Concurrent desktop interaction
prevented completion of the manual keyboard and drag sequence.


## Surface refinement

The reasoning popup now puts the track and current value on one row without a
header. Model selection has its own 176-point width. The composer and popovers
use soft tonal surfaces and shadows instead of full outlines. The primary button
uses a restrained gradient and a solid, readable Live waveform. Workspace panel
headers use a subtle fill rather than redundant horizontal rules. Native window
transparency remains unchanged. Strict Clippy passed for these presentation edits.


## Popover interaction correction

Reasoning drag events now belong to the popup's window event handlers. The slider
sets keyboard focus on pointer down. The former workspace-only listener did not
cover the detached popup. The replacement regression begins at the rendered track
bounds, moves beyond both ends, releases, uses a direction key, switches to the
model menu, and clicks the conversation to dismiss it. It uses a multi-level model
catalog rather than preloading drag state with a single fallback level.

Model and effort popovers share a stable width. A short opacity transition replaces
height disclosure, so switching cannot stretch or crop one menu into the other.
Outside clicks dismiss the popup while clicks on its triggers retain toggle behavior.
The popup uses a flat contrasting surface, softer corners, a filled reasoning track,
and a plain checkmark for the selected model. Decorative gradients were removed.

Validation: 170 GPUI tests passed, five opt-in tests ignored, and strict Clippy passed.

Native acceptance: dragged the thumb from High to Ultra, used Left three times
to restore the user's Medium setting, switched to the model menu, and clicked
the conversation to close it. Accessibility readback confirmed each result.
Screenshots confirmed both popup layouts. One preview and its owned service
remain open; no message or audio session was started.


## Continuous motion and history alignment

The reasoning thumb follows the pointer continuously while its semantic value
snaps to supported levels. Release clears pointer state, requests a frame, and
eases the thumb to the selected stop. The compact control uses a quiet track and
rounded rectangular thumb rather than the previous wide colored capsule.

The menu shell owns its shadow. Opening, closing, and replacement use interruptible
opacity and size transitions; content fades independently. The primary action no
longer stacks a second shadow. Workspace panel separators use spacing and material
changes; graph dependency edges and selection indicators remain meaningful.

History navigation and active-mark detection now share the same top inset. A
clicked turn remains selected even when bottom clamping prevents its anchor from
reaching that inset. Manual wheel scrolling releases that selection. The rail's
active emphasis interpolates in position, width, and opacity. Marks are painted
around their actual hit-target centers. Jump destinations are remeasured during
navigation so layout changes do not leave a stale target.

Validation: 172 GPUI tests passed and five opt-in tests were ignored. Regression
coverage includes actual rail hit targets, consistent anchor detection, continuous
pointer positions, release state, keyboard adjustment, menu replacement, and outside
click dismissal. Strict Clippy passed.

Native acceptance: inspected the final glass workspace, loaded earlier real
history, jumped to the long project-check request, scrolled through adjacent
turns, and returned to the latest turn. The reasoning slider was dragged to Ultra
and keyboard-adjusted back to Medium. Popup switching and outside dismissal were
exercised. Removed the trial panel tint after screenshots showed it darkened the
glass regions. One signed preview remains open with no draft or active recording.

## Consistent disclosures and glass panel contrast

The reasoning value has a fixed, non-shrinking single-line label. The slider has
one tick for each supported level and a rounded capsule surface. Pointer movement
remains continuous; release settles onto a supported level.

Composer menus now use one 140 ms opacity transition for opening, closing, and
replacement. Removed measured-height and translation effects, which made initial
and subsequent openings behave differently. The shell owns the surface and shadow.

The tree and graph use a light translucent overlay to distinguish their regions
from the conversation without hairline dividers. A weaker overlay was not visible
enough in the signed native window; the final overlay was checked there.

Validation: actual slider drag selected Medium; model replacement and outside
dismissal worked. Inspected the signed preview at its normal window size.
The GPUI suite passed 173 tests (five opt-in tests ignored); strict Clippy passed.

## Share the sidebar glass composition

Removed the white tint experiment from the tree and graph. They now share one sidebar material on their parent over the shell glass,
with no additional per-panel tint. The conversation alone adds a small overlay.
This also prevents untinted gaps at the conversation corners and control strip.

Native review and the user's screenshot showed that the original conversation
tint then formed a large dark rectangle. The Chief conversation now uses the same
hue with a small opacity difference and 14 px corners. The left sidebar material
is unchanged.

Reduced the reasoning disclosure from 60 px to 38 px high. The track is 4 px,
the thumb is a 12 px circle, and supported-level ticks remain. Composer, menus,
and the voice/send control now use neutral gray instead of separate blue tints.

Validation: reviewed the final signed native workspace and compact popup; dragged
the slider to Medium. The shared glass no longer exposes bright corner seams.
The GPUI suite passed 173 tests (five opt-in cases ignored); strict Clippy and the
final material checks passed. One preview remains open with an empty draft.

## Combined model and reasoning disclosure

One toolbar control now shows the compact model name and reasoning level.
Its popover contains the model list and the slim reasoning slider. Selecting a
model or reasoning level keeps the popover open. Model changes reconcile supported
reasoning levels and Fast capability through the existing capability logic.
Clicking the trigger again, clicking outside, or pressing Escape closes it.

Validation: the selection regression keeps the combined menu open while reconciling
model capabilities. The native event regression checks slider drag, keyboard
adjustment, trigger close/reopen, and outside dismissal. All 173 GPUI tests passed
(five opt-in cases ignored), and strict Clippy passed.

Signed-app acceptance: selected Sol with the menu still open, returned to Astra,
dragged reasoning to Medium without closing the menu, and closed it with Escape.
Inspected the combined menu screenshot. The preview has no draft or recording.

## Compact selector and disclosure motion

Reduced the combined selector width from 264 px to 232 px. The toolbar uses a
compact model / middle-dot / reasoning label with 2 px gaps. Disclosure motion
uses one interruptible 180 ms progress value for opacity and a 4 px vertical
offset. It does not animate measured content height or use independent clocks.

Validation: all 173 GPUI tests passed (five opt-in cases ignored), including
transition reversal and native slider / dismissal events. Strict Clippy passed.

Signed-app readback confirmed closing, reopening, and model selection that keeps
the popup mounted. The native screenshot showed the compact toolbar label;
subsequent popup captures returned white images, so those captures do not establish
a complete visual animation review. The previous project conversation was restored.

## Shared opaque popover surfaces

Composer menus and the bottom-right status panel now share the same popover
component. The surface is opaque neutral gray when open. Window and workspace
glass materials are unchanged. Both use the same 14 px radius, shadow, and
interruptible 180 ms fade with a 4 px lift. The status panel no longer animates
its measured content height. Each popover has an independent animation identity.

Validation: 173 GPUI tests passed (five opt-in cases ignored); strict Clippy passed.

Signed-app acceptance: inspected both opaque panels, switched from Status to the
model selector, and verified outside dismissal and Escape. Restored the open
project and worker tabs. One preview remains open with no draft or recording.

## Ordering, feedback, and popover shadow lifetime

The model menu groups GPT releases in descending numeric version order, preserving
catalog order among variants of the same release. Unknown model families retain
their relative catalog order after recognized GPT releases.

The reasoning footer no longer has a separate rounded surface. It contains the
slim slider and Fast toggle; the combined toolbar label shows a lightning mark
when Fast is on. Model rows have a clearer neutral hover fill. Slider hover/drag
animates track thickness and thumb size without changing its hit bounds.

Popover content and its shadow-bearing surface now remain mounted together until
the shared fade ends. The complete surface is absent at zero progress. Shadow
strength additionally follows progress so low-opacity opening/closing frames do
not leave a detached-looking shadow. The GPUI revision already multiplies painted
shadows by element opacity; no upstream opacity patch is needed. The other two
application shadow sites belong to persistent composers, not disappearing popovers.

Validation: 174 GPUI tests passed (five opt-in cases ignored), including numeric
version sorting, same-version stability, and slider interactions. Strict Clippy
passed.

Signed-app acceptance: confirmed GPT-6 first, GPT-5.6 variants together, then
GPT-5.5; toggled Fast on/off without closing; dragged to Medium; selected Sol
and restored Astra with visible row feedback. Inspected the final native popup
and slider screenshots after intermittent white captures. Notification and
selector dismissal were exercised. Open project/worker tabs were restored.

## Native pointer tracking

Register a visible-bounds AppKit tracking area on the GPUI drawing view. Route
plain movement and exit events to that view even when its first responder changes.
Keep cursor selection in GPUI hit testing; do not assign a hand cursor to the
window or popover surface. Registration skips an existing matching area.

The model-menu capture fixture now includes an unselected model. A button-free
GPUI mouse move visibly highlights that row. This capture explicitly redraws the
window, so it proves styling and hit testing, not native event delivery. A native
Swift test also verifies that preparing the voice host preserves the window's
mouse-move setting, first responder, and mouse event delivery.

Validation: 174 GPUI tests passed (five opt-in cases ignored); strict Clippy passed.
The native pointer-preservation test passed. Physical pointer hover and cursor
reset in the signed preview still need confirmation; the native UI driver exposes
click and drag but no button-free pointer move.

## Status dismissal stacking

Render the global status center through GPUI's deferred overlay pass at priority 3,
above the composer menus at priority 2. Keep this wrapper mounted while the shared
popover finishes its closing animation. Previously the status center was an
ordinary child of the shell, outside the popup drawing order.

The visual capture tool can now show status over any workspace fixture with
`DECODEX_VISUAL_STATUS=1` and sample a click-driven dismissal with
`DECODEX_VISUAL_STATUS_CLOSE_MS`. Inspected closing frames over the normal composer
and Live button. These captures show the panel fading across its bounds without a
separate cutout; they are sampled frames, not exhaustive native animation evidence.

Validation: 174 GPUI tests passed (five opt-in tests ignored); strict Clippy passed.

## Separate model card and reasoning capsule

The model selector uses two opaque surfaces: the model list and a separate rounded
reasoning/Fast capsule. An 8 px gap exposes the workspace between them. One
unframed popover owns their opacity, offset, lifetime, and outside-click boundary,
so both surfaces open and close together. Attachments and Status retain their
existing single-surface treatment.

The history rail now uses selection only for color. Width and height changes come
from pointer proximity; the selected mark returns to normal size after hover ends.

Validation: strict Clippy and the slider drag/model-selection/outside-dismiss test
passed. Inspected the rendered model-card/capsule layout in the visual harness.

## Composer proportion audit

Reviewed the closed composer and all five model options in the native window.
Keep the 232 px model card, 32 px model rows, current typography, and 28 px action
targets. Align the model popover's right edge with its trigger by accounting for
the trailing microphone/send controls. Reduce the reasoning capsule from 38 px to
36 px. Use a 6 px gap within the pair and 10 px below it to clarify grouping.
Raise Fast-off opacity from .45 to .65 so off is not mistaken for unavailable.

Strict Clippy and the existing slider/model/outside-dismiss interaction test pass.

## Tooltip restraint

Remove redundant composer tooltips from the labeled model trigger, attachment row,
microphone menu row, and back row. Workspace text actions no longer all receive
identical label tooltips; retain icon help, potentially truncated page/sidebar
names, and pending-decision context. Remove duplicate Retry, Send, Stop, and vague
recovery tooltips from the conversation surface. Keep accessible labels and hover
feedback. Delivery help now states when Steer or Queue sends the message.

Retain context usage details, history previews, graph names/blockers, quote
attribution, icon help, and shortcuts. Strict Clippy passes. Plain-pointer tooltip
appearance was not exercised by the native UI driver, which has no hover command.

## Restore the native text responder on editor clicks

A GPUI focus handle does not reassign AppKit's first responder. An editor click now
claims the owning GPUI NSView as the native text responder before updating GPUI
focus. This lets keyboard text reach the input client after native focus moved away.
Do not bypass IME composition or insert raw key characters as a fallback.

The native visual harness can clear AppKit focus and click the real composer with
`DECODEX_VISUAL_NATIVE_INPUT_FOCUS=1` and the `composer` fixture. Its responder
assertion fails when the new call is removed and passes with the fix. The existing
composition/undo and newline/caret tests also pass. Strict Clippy passes.

The signed preview includes the fix. The user confirmed physical keyboard input
works for both English and Chinese. Earlier synthetic key/paste checks did not
cover this failure and were not sufficient acceptance evidence. The observed
InputMethodKit log message alone did not establish an input-method fault.

## Standard macOS draft editing keys

Add Command-Backspace/Delete for deletion to the displayed line boundary,
Option-Backspace/Delete for word deletion, Command-Left/Right for line navigation,
Option-Left/Right for word navigation, and Shift variants for selection. Add
Command-Up/Down and their Shift variants for draft boundaries and Shift-Up/Down
for vertical selection. Keep Enter submission and Shift-Enter newline behavior.

Use the existing locked unicode-segmentation release for Unicode word boundaries
and grapheme-safe character movement/deletion. Deletions keep the existing undo
history and delete a selected range before applying boundary expansion.

Validation: four input tests pass, covering keyboard dispatch, multiline deletion,
selection, undo, IME composition, wrapping/caret geometry, and composed graphemes.
Strict Clippy passes. No new input mode or visible controls were added.

## Inline microphone disclosure

Keep microphone selection inside the attachment menu. Expand a bounded device list
under the existing microphone row; collapse to that row without a back page. Use a
single animated up/down chevron and one popover identity, so disclosure does not
restart the whole menu's entrance animation. The plus trigger closes either state.
Device selection keeps the menu open for confirmation.

Align the three main rows at 32 px with a shared icon gutter, label column, value
column, and trailing chevron slot. Use the shorter `Send mode` label. Align device
names and checkmarks with the microphone header, and bound long lists with scrolling.

Validation: inspected the expanded visual fixture; 176 GPUI tests passed (five
opt-in cases ignored), and strict Clippy passed before the final 2 px alignment
adjustment. The visual fixture uses explicit example device names only in capture
and test builds; the running app still discovers native devices.

## Read-only quote tooltip

The quote tooltip now contains only the full quote and author. Move the ZenQuotes
attribution link to the General settings footer, where pointer and keyboard
activation are supported. This replaces the earlier hover-only attribution link.
Strict Clippy passes.

## Remove the redundant quote tooltip

Remove the quote hover window entirely, including its unused lookup and placeholder
getter. The placeholder already presents the quote and author. Keep the source link
in General settings. Strict Clippy passes.

## Conversation spacing refinement

Keep the existing glass materials, composer, and panel layout. Reduce assistant
message vertical padding from 8 px to 2 px; the history list already separates
messages by 20 px. Apply the same padding to streaming replies. Reduce user bubble
vertical padding from 12 px to 9 px. Set reply metadata to a 15 px line height with
a 6 px gap from the reply, so it reads as part of that message.

Validation: inspected native-rendered Markdown and continuous-conversation
fixtures, including a before/after Markdown comparison. User bubbles stay right
aligned and separate messages remain distinct. Strict all-target, all-feature
GPUI Clippy passed. These offscreen fixtures do not validate desktop vibrancy.

## Floating controls and shared material

Remove the full-width conversation-tab tint and underline. Use a small neutral
selection surface instead, and use the same light selection tint in the project
sidebar. Group the top-right controls with the existing floating toolbar material.
Order them as history rail, bottom graph, right agent tree, and settings; keep all
four hit targets at 24 px inside the 28 px group. Existing shortcuts are unchanged.

Use neutral smoky graph nodes with softer corners and a visible hover tint. Keep
the existing glass plane and composer material. Inspect the native-rendered
workspace fixture and run strict GPUI Clippy before packaging.

## Remove the invisible composer footer boundary

Overlay the composer on the full-height transcript instead of reserving a
full-width footer that clips messages above the capsule. Only the capsule blocks
pointer hit tests. Measure the footer, including draft growth and activity, and
reserve that height at the end of the transcript so the final reply can scroll
clear of it. Preserve the capsule's outer spacing and shadow.

Validation: inspect the multiline composer capture; assert the transcript extends
behind the composer in the real GPUI wheel-scroll test. Use a shorter fixture
window so its content still supports the full 100 px scroll delta after the
viewport expands. GPUI tests: 176 passed, five opt-in tests ignored.
