# Current-turn model control

## Scope and ownership

This is an optional explicit control. A user reviews a running task, selects a
model and reasoning effort, then chooses **Apply to this turn**. Decodex checks the
current account catalog, exact process/account/thread/turn source and native
`step_model_switching` feature before it reserves and sends the edit. The control
does not enable that feature or select a replacement task.

Native `turn/settings/update` owns subsequent captures in the running turn.
`thread/settings/update` owns saved defaults for later turns. An applied receipt
confirms publication; it does not prove that a later inference occurred. Pending
tools and approval requests keep their original state.

The restored service uses the shared live-settings journal from PR1567 and the
transport from PR1566. Reviewer and model edits use one review/reservation
sequence. Unknown replies are recorded and never automatically retried. A receipt
refresh does not authorize a second click. Source changes, disconnects and native
child navigation invalidate the desktop review.

Local protocol 2.92 adds the model action, requested catalog query and model
receipt fields. It remains an exact-version local protocol. Database schema 48
is unchanged. No production configuration or automation is enabled.

## Upstream and installed-native evidence

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The protocol and processor implement exact-turn publication. The native
`turn_settings_update.rs` suite distinguishes A/A/B saved-default edits from
A/B/A current-turn edits. `ExperimentalFeatureListParams.thread_id` reads the
loaded task's effective feature configuration.

Installed Codex 0.158.0-alpha.2 passed isolated loopback fixtures with private
HOME and CODEX_HOME, no account credentials and no remote model service:

- A held tool starts with `gpt-5.6-sol/low`. Publication does not release it. The
  next capture uses `gpt-5.6-terra/high`. A cold next turn uses `gpt-5.6-sol/low`.
  Saved defaults, the config file and durable applied receipt remain correct.
- With the native feature disabled, the same selection is rejected. All three
  captures retain `gpt-5.6-sol/low`, and no edit reservation is created.
- An absent catalog model is rejected before reservation in both cases.
- A new native child inherits the updated captured model and effort.
- Native spawn descriptions follow the updated model's catalog entry. The old
  alpha.16.3 limitation does not reproduce on the installed alpha.2 build above.

The native fixture closes the bridge, waits for child termination and joins its
reader. The existing native reviewer scenario remains a separate regression check.

## Validation and remaining acceptance

Rendered socket tests cover explicit model selection, exact-turn submission,
lost-response readback, no replay and source/child/disconnect invalidation.
Capability tests check task-scoped feature queries and catalog projection.
Protocol tests cover exact-version admission and current wire golden messages.
Strict all-target, all-feature checks cover protocol, runtime and GPUI.

These checks do not constitute signed desktop visual acceptance, installation or
a release. The shared signed artifact and lifecycle acceptance remain open in
R07/R12. The shared production modules retain their separate reconciliation entries.
The inherited wire-test helper is mapped below.

## Removal decision

The user can remove this optional control without removing saved-task model
selection or native approval enforcement. Remove its UI/action/service publisher
together, then remove unreferenced transport code. Keep historical journal records
readable and retain the reviewer sequence. No removal is part of this update.

## Inherited socket fixture reconciliation

The complete inherited `chief_live_settings_wire_tests.rs` differs from the
current file in these ways:

- The reviewer and current-turn model modes use the same private Unix socket
  fixture. Keep the exact task, turn, review token and action assertions. Restore
  the model-mode guard and the inherited assertion that lost-response readback
  retains the selected model and reasoning effort.
- The old task-model modes moved to `chief_models_wire_tests.rs`. Its four-case
  fixture covers explicit or preserved effort and pending or observed readback.
  It asserts one `SetTaskModel` command, preserves the input draft and invalidates
  the old model observation. The separate rendered history case preserves
  restart reconciliation without claiming that unknown delivery succeeded.
- Task defaults now use `GetChiefModelSelection` and the current flat action
  fields. Current-turn settings still use `GetChiefLiveReviewer` and
  `SetLiveModel`. Private helper visibility replaces the former shared helper;
  it does not remove a product entry point.
- Current tests add a fresh-review requirement after receipt readback, reject a
  repeated click and invalidate edits on child navigation. Disconnect now uses
  the actual `apply_result` path. The existing task/thread/turn/source transition
  test remains present.

This closes only the inherited socket-test file. It does not close shared
production-file review, native binary limitations or signed desktop acceptance.
The changes restore test evidence; they do not change runtime behavior.

Fresh validation: four current-turn socket tests and six task-model socket/rendered
tests pass. Strict GPUI Clippy passes for all targets and features. Evidence:
`/tmp/decodex-live-settings-receipts-tests.log`,
`/tmp/decodex-live-settings-task-mapping-tests.log` and
`/tmp/decodex-live-settings-receipts-clippy.log`.
