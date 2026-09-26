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
R07/R12. The inherited wire-test helper also included saved-task model cases;
that file and other shared modules retain their separate reconciliation entries.

## Removal decision

The user can remove this optional control without removing saved-task model
selection or native approval enforcement. Remove its UI/action/service publisher
together, then remove unreferenced transport code. Keep historical journal records
readable and retain the reviewer sequence. No removal is part of this update.
