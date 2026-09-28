> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Signed desktop acceptance at 0658d23f

## Artifact and fixture

On 2026-09-27, the repository stage script built and signed the normal desktop
application from clean commit 0658d23f1cf4018beb06a31e3cdc81831ddcfde5.
The bundled helper reports that exact commit and dirty=false. The stage script
passed its bundle contracts, signing-team checks and deep strict signature check.
This was local staging, not notarization, installation or release.

Artifact:
`target/upstream-0658-acceptance-stage/Decodex.app`.
The production source is equal to main caa59c5e3aa3b7e205d5732ed4687cc25c346814;
the intervening changes are tests and review records.

The existing public-socket desktop fixture used an isolated HOME, the real
service and installed Codex 0.158.0-alpha.2.1, with synthetic loopback model
responses. The fixture owned the service across GUI exits. This does not qualify
app-owned service startup/shutdown or a cold service restart.

## Observed interactions

The exact staged application window was readable through CUA and screenshots.
The visible fixture prompt and isolated warning path established the selected
test context. The earlier window-not-found observation did not recur; its cause
has not been established.

- Switch from native history to saved local records and back.
- Open Task recap and explicitly generate a result. The controls move from
  Cancel to Refresh/Generate, and the synthetic summary is visible.
- Open Model and reasoning, enter fixture-custom-model in Exact model ID,
  and apply it to the next turn without sending input.
- Enter the unsent text “Draft preserved across signed desktop restart.”
- Quit from the native application menu, then relaunch through the fixture.
- Read the same task and native answer after relaunch. The exact draft text,
  fixture-custom-model and provider-effort caption are restored.
- Open the saved recap without a new model request, then quit normally again.

The fixture records two launches, two successful exits, the same parent thread
and two model requests: the initial reply and the explicit recap. Its final
interactive test passes in 492.89 seconds. A finish marker verifies fixture
process completion; it does not certify every acceptance requirement.

## Open observations

The expanded recap begins above the visible clip. CUA scrolling did not establish
full readability. A new rendered test uses the complete production workspace and
a valid multiline recap within the 700-character bound. Both pixel and line
wheel events cancel latest-follow and bring the recap heading into the viewport.
This test passes without a production fix. The physical-window observation is
still unresolved; do not call it a proven product defect or completed acceptance.

Initial coordinate model clicks had no observable effect. The later accessible
Model and reasoning control worked. These attempts do not prove every native
overlay or accessibility path.

After the second Quit, a subsequent accessibility read started an additional
staged-app process outside the fixture runner. Stop UI actions in that context.
The exact extra task-artifact PID was checked and terminated; absence was then
verified. Its HOME scope was not verified. The production application was
observed running after cleanup. Do not infer why earlier production PIDs changed,
or claim that the extra launch had no possible state effects.

After quitting in future fixtures, use the fixture's process record for exit
confirmation. Do not inspect or reacquire the exited app binding: that can launch
an app outside the isolated fixture. The stale CUA binding was released.

## Evidence and remaining scope

Local evidence is retained under
`/Users/x/.decodex-0658-desktop-wn4phc5r/`, including acceptance-results.json,
desktop-ready.json and post-quit-tool-relaunch.json. Stage and runtime logs are
`/tmp/decodex-upstream-0658-stage.log` and
`/tmp/decodex-0658-desktop-run.log`. Screenshots are in the task conversation;
no image files were exported.

The current 13-test GPUI recap suite and strict all-feature/all-target Clippy
pass. Logs are `/tmp/decodex-recap-current-validation.log` and
`/tmp/decodex-recap-current-clippy.log`. These tests do not replace the unresolved
physical scroll check.

Foreground/background recap, opt-out, live voice, all approval and media flows,
blank-task/worktree behavior, Dock activation, conflict cancellation, export
and app-owned service lifecycle remain outside this run. R06/R07/R12 remain open.
Maintenance automations stay paused.
