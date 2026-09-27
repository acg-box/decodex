# Reconcile the task model panel

The inherited task-default selector is implemented by the current `chief_models`
panel and its socket tests. The complete inherited panel and test file were read.
The optional task model control remains separate from explicit next-message
choices and current-turn model changes.

| Inherited behavior | Current owner and evidence |
| --- | --- |
| Review exact task, thread and runtime source before sending | `model_selection_action`, `update_task_models` and `finish_task_models`; foreign bindings, child navigation, disconnect and late results are rejected. |
| Select a catalog model and an advertised effort | `task_model_choices`; selecting a model exposes explicit effort actions. A separate explicit action can preserve effort, including for catalogs that report no effort list. |
| Send a task-default command once and read its receipt after a lost reply | The four-case socket fixture checks the exact `SetTaskModel` payload, one write, pending and observed readback, and the unchanged input draft. It never calls the live-turn method. |
| Retain original response, manual or automatic origin, target observation and restart reconciliation | `history_label` and the rendered receipt tests. Reconciliation does not assert successful old delivery. A fresh review remains required before another edit. |
| Discard draft choices across task, turn and source changes | The transition test restores the old snapshot after each change and verifies that the old review and selected model do not return. |
| Invalidate the configured model observation after a write | Restore the missing call through `reset_model_settings`. It clears cached observations and invalidates reads by epoch. Explicit next-message choices retain their separate owner. |

The final row was a real regression. The socket fixture first reads a native
`previous-model` observation, then submits a task model change. Before the fix,
the composer still reports `previous-model` after command readback. This fails in
`/tmp/decodex-model-observation-refresh-before-assertion.log`. Both lost-reply
outcomes must invalidate that observation; neither is permission to invent a
successful model change. A later native read supplies the current observation.

Keep the existing task-model service, journal and public command. No second
selector, native mutation owner, schema or protocol is added. The new UI requires
a fresh review after a write, even if its readback is reconciled. This is an
explicit review step, not a loss of the ability to edit a reconciled task.

Close only the two inherited task-model panel and panel-test file rows after
focused rendered socket tests and strict GPUI validation. The shared model
observation module and full runtime/database writer review remain separate.
These are source and fixture results; signed desktop visual acceptance remains
open. The model control stays optional in the user's removal review.

Validation passes all six rendered model-panel tests, including the four socket
cases, and strict GPUI Clippy with all features and targets. Final logs are
`/tmp/decodex-model-observation-refresh-final.log` and
`/tmp/decodex-model-observation-refresh-clippy-final.log`.
