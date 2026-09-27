# Signed desktop artifact at a15fe830

## Verified artifact

The repository stage script completed from clean signed commit
`a15fe8303e0350b9c668a62f783d00790a220462`. Its bundled helper reports that exact
commit and dirty=false. Bundle contracts, signing-team checks and deep strict
signature verification passed. This is local staging, not installation,
notarization or release. Two existing release-build warnings remain; this record
does not claim a warning-free build.

Artifact: `target/upstream-a15fe830-acceptance-stage/Decodex.app`.
Build log: `/tmp/decodex-upstream-a15fe830-stage.log`.
This artifact includes the later source/relationship and catalog repairs missing
from the older 0658 acceptance artifact.

## Initial app-owned service observation

Launch the exact staged executable with a new private test home and valid local
profile. No account is enrolled. The app PID 94553 directly creates bundled
service PID 94556 with serve --parent-fd 6. A public status read from that private
root reports configuration, SQLite, protocol 2.95, server identity and vault ready.
Conversation authentication is unavailable, as expected for the empty fixture.

The UI tool cannot select the exact staged bundle: two getApp calls return
cgWindowNotFound. Inventory shows multiple Decodex entries but no usable exact
window identity. The documented optional launch_app API is unavailable on this
host and performs no action. Do not substitute another running Decodex instance.

After this failure, verify the fixture app's exact executable and send SIGTERM
only to that PID. Both app and its owned service are then absent. This observes
parent-exit service cleanup; it does not prove normal menu Quit, draft flush,
Dock reopening, an active native turn or graceful shutdown completion.

Fixture record:
`/Users/x/.decodex-a15-owned-cwilue1q/lifecycle-results.json`.

## Initial native conversation fixture

Compile the current runtime test artifact and run the existing isolated public
socket desktop fixture. It reaches desktop-ready with PID 95061, one native model
request and thread `01a0e274-a6dc-71f3-8f05-459761bd8a10`. The provider is synthetic
and local. This differs from the empty-profile case and tests whether initialized
conversation state makes the exact window accessible.

The UI tool again returns cgWindowNotFound for the staged bundle. No desktop
interaction is accepted. Stop the verified fixture app deliberately. Its normal
exit assertion then fails, and the test completes with exit 101. This is an
aborted acceptance fixture, not a passing test or a reproduced application crash.
Readback finds no remaining processes for the two fixture roots or staged GUI.

Evidence:

- `/Users/x/.decodex-a15fe830-desktop-xndpad0o/desktop-ready.json`
- `/Users/x/.decodex-a15fe830-desktop-xndpad0o/ui-tool-blocker.json`
- `/tmp/decodex-a15fe830-desktop-run.log`
- `/tmp/decodex-a15fe830-desktop-current.json`

## Restored window access and normal exits

After the user reported restored access, the UI tool selected the exact staged
bundle. The previous cgWindowNotFound observations above are historical.

In the empty-account fixture `/Users/x/.decodex-a15-owned-kka2yj3e`, the UI menu
Quit exited GUI PID 8251 with status 0. Its recorded bundled service children,
PIDs 8256 and 8269, were absent after exit. The fixture's lifecycle-results.json
records these observations. This qualifies normal GUI exit and owned-service
cleanup for an empty profile. It does not qualify shutdown during active work.

The initialized native fixture at
`/Users/x/.decodex-a15fe830-desktop-i7b9p91b` completed two normal GUI exits and one
relaunch. A recap action increased the local synthetic provider request count
from one to two. The recap text was not visibly confirmed, so this is request and
exit evidence, not complete recap acceptance. The fixture test passed. Its
service remained owned by the test across GUI launches.

## History-edit draft persistence

The next fixture at `/Users/x/.decodex-a15fe830-desktop-ckykp4ty` provides a stronger
restart result. Select Review earlier input in the connected native conversation.
The canonical input appears in the review field. Set its unconfirmed edit text to
`Unsent history edit ckykp4ty`; do not confirm the history edit or send input.

After normal menu Quit, the private draft file retains the text. Relaunch the
same fixture: the Edit draft control and reopened review field both show the
exact text. The original prompt and Saved native answer remain visible. A second
normal menu Quit completes the test. The fixture records two launches, two
successful exits and one model request throughout: restart did not replay input.
The private draft-acceptance.json records the exact native thread and observations.
The isolated native fixture test passes without ignored or failed cases.

This proves persistence of an unconfirmed history-edit draft. It does not prove
ordinary-composer draft recovery, conflict resolution or export.

## Current interaction limit

The main window can be raised with its exposed accessibility action. Its native
history and review controls are then available. The ordinary composer is rendered
in a separate native child window owned by chief_native_composer.rs; it is absent
from the observed main-window accessibility tree. Coordinate and keyboard focus
attempts did not establish input in that child. The Window menu exposed only the
main Decodex entry. Do not classify the underlying draft behavior as passed or
failed from this tool boundary.

A separate probe at `/Users/x/.decodex-a15fe830-desktop-stl6a0vr` ended with one
normal GUI exit and one provider request. Its test passed, but no composer input
was verified. These fixtures use private homes and a local synthetic provider;
they do not establish physical audio or real-provider behavior.

## Remaining acceptance

Ordinary composer input and restart, foreground/background recap, opt-out, live
voice, Dock reopening, draft conflicts/export and active-work shutdown remain
unverified to their required scope. Existing approval/media and rendered/native
fixture results remain separate evidence. Main-window access is restored; the
remaining child-window interaction limit is narrower than the original blocker.
No automation is enabled, and the overall manual update remains incomplete.
