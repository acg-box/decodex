> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Model display and control reconciliation

Read the complete inherited and current model-observation and composer-control
files. Verify both preserved file hashes. The current owners retain the inherited
behavior with the following explicit changes.

## Model observations

`chief_model_settings.rs` uses separate observations for the composer owner and
inspected task instead of one shared slot. Before each read it retains only those
two task keys. Inspecting a worker cannot replace its manager's model label.
The two-second background refresh limit does not prevent an explicit refresh.

The display prefers explicit next-message intent, then the current native
observation. Catalog names label exact model IDs; a missing catalog retains the
exact ID. New-task reasoning inheritance has an explicit label. An observation
never changes model intent. It can update the displayed effort only while the
input, owner and intent revision still match and no explicit effort was selected.

The reader retains the same source, generation, selected task, native thread and
reply-identity checks. An epoch rejects an old read after reset. Source, thread,
turn, dispatch or task-removal changes clear the observations. A failed or stale
service also clears them. Closing independent details does not clear them.
The read view retains its previous observation while refreshing; its copy states
that these are last-read task settings, not per-turn execution history.

The old task-selection reset and invalidation calls moved to their current
`reset_task_models` and `invalidate_task_models` owners. The profile, workspace,
failed-snapshot and successful-snapshot paths call these owners separately from
model observations. The task-model write path clears model observations after
readback, as qualified in [the selector mapping](task-model-panel-reconciliation.md).
There is no second selection or native settings authority.

Six current tests cover actual socket reads, foreign replies, A-B-A source
changes, later user intent, model-specific efforts, independent worker inspection,
stale service and detail-close behavior. All six pass. These preserve core
consistency if the optional model controls are retained.

## Composer controls

`chief_composer_controls.rs` retains the inherited effort labels, glyphs, catalog
buttons, keyboard actions and stable release ordering. Selection compares the
exact model value, so a catalog display name cannot change which entry is marked.
The removed compact-label helper has no remaining caller; the composer uses the
native catalog name and falls back to the exact ID. The existing composer tests
verify renamed labels independently of the selected model.

Current stop-confirmation and primary-action glyphs are additive presentation
changes. Preserve their existing fixed footprint and motion owner. The restored
[exact model field](exact-model-input.md) sits beside catalog selection, for both
new and existing tasks. It uses the same explicit selection owner and does not
submit. All 15 composer tests pass, including catalog ordering, exact-input
clicks, drag/dismiss controls and draft ownership.

Close these two complete inherited file rows only. Keep the larger composer,
workspace, surface and signed desktop reviews open. These tests verify rendered
controls and state contracts; they do not establish packaged visual acceptance.
The prior signed package predates the exact-model field.
