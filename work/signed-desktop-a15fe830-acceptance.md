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

## Deferred composer interaction

The main window can be raised with its exposed accessibility action. Its native
history and review controls are then available. The ordinary composer is rendered
in a separate native child window owned by chief_native_composer.rs; it is absent
from the observed main-window accessibility tree. Coordinate and keyboard focus
attempts did not establish input in that child. The Window menu exposed only the
main Decodex entry. A later isolated fixture exposed a Decodex Composer window
with a settable Chief message field. This supplies a possible interaction route;
it does not establish a passing input test. The user deferred this work until
the other independent acceptance steps finish.

A separate probe at `/Users/x/.decodex-a15fe830-desktop-stl6a0vr` ended with one
normal GUI exit and one provider request. Its test passed, but no composer input
was verified. These fixtures use private homes and a local synthetic provider;
they do not establish physical audio or real-provider behavior.

## Automatic recap opt-out

In `/Users/x/.decodex-a15fe830-desktop-_89two_4`, use the signed Settings window to
change Automatically recap tasks from off to on and then off. Read-only database
observations confirm auto_recap=1 at revision 2 and auto_recap=0 at revision 3.
After normal Quit and relaunch, the Settings control remains off. A second normal
Quit completes the fixture with two launches, two successful exits and exactly
one model request. The test passes in 120.08 seconds.

Evidence: `recap-optout-acceptance.json` in that fixture home. This verifies the
setting and persistence of the disabled state. It does not verify the real
30-minute background trigger or cancellation of an active recap.

## Close the main window

In `/Users/x/.decodex-a15fe830-desktop-_se9357r`, open Settings, select the main
Decodex window, then use File > Close Window. GUI PID 20071 remains alive and
Settings remains visible. The fixture still has zero exits and one model
request. Normal Quit from Settings then produces one successful exit with the
same request count. See `window-close-acceptance.json` in the fixture home.

This verifies that closing the main window can retain the application and its
Settings window. The Window menu does not list the hidden main window in this
observation. Dock reopening remains unverified.

## Shutdown during an active native request

The installed Codex binary is 0.158.0-alpha.2.1, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
An isolated local provider holds a real /responses HTTP connection open after
SSE headers. Before shutdown, the probe observes the exact native turn/started,
an active process and no turn/completed. Closing native stdin produces exit 0
in 0.018 seconds; the provider observes disconnect and exactly one request.
Evidence: `/private/tmp/decodex-active-shutdown-3it104xy/result.json` and the
adjacent native-messages.json and stderr.log.

The Decodex service fixture adds a separate boundary. Its opt-in
DECODEX_TEST_ACTIVE_SERVICE_SHUTDOWN mode requires a public Running work snapshot
with an active native turn and the provider-started witness. ProtocolServer
shutdown succeeds, the provider observes disconnect, and the request count
remains one. The final-source test passes in 12.60 seconds. Evidence:
`/Users/x/.decodex-active-service-5h5w3f2v/active-shutdown-before.json`,
`active-shutdown-result.json` and the provider witness files in the same home.
The ordinary socket regression passes in 16.65 seconds; strict runtime Clippy
passes with all features and targets.

These observations verify native EOF and service shutdown with an active model
request. They do not verify normal GUI Quit during active work, tool-child
cleanup or every Guardian retry and cached-allow race.

## Native unload and resume

The isolated installed-native probe uses thread_unload_delay_secs=2, a supported
test configuration in the fixed upstream source. After one completed turn,
unsubscribe leaves the thread loaded. A warm resume cancels the pending unload;
the thread remains loaded after 2.3 seconds. A second unsubscribe produces
thread/closed after 2.02 seconds and removes the thread from the loaded list.
Cold resume preserves the thread identity and Saved native answer. The provider
request count remains one, and native EOF produces exit 0.

Evidence: `/private/tmp/decodex-unload-resume-b34cfziw/result.json` and its adjacent
native message records. This verifies actual idle unload, warm cancellation and
cold history recovery without replay. Pending-unload/revert replacement races
remain outside this probe.

## Remaining acceptance

Ordinary composer input and restart, complete foreground/background recap, live
voice, Dock reopening, draft conflicts/export and GUI Quit during active work
remain unverified to their required scope. The real 30-minute background fixture
is in progress and is not a passing result. Existing approval/media and
rendered/native fixture results remain separate evidence. Main-window access is
restored; the composer interaction work is deferred by user instruction.
Maintenance automation remains paused, and the overall manual update remains
incomplete.
