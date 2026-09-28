> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile retained settings surfaces

Full preserved/current diffs account for four inherited files. No product code
change is required in this batch.

| File | Complete disposition |
| --- | --- |
| `chief_permissions.rs` | Retain the original exact work/thread/token/profile checks. Action construction moved to `permission_selection` and the canonical action is now `SelectPermissions`, routed to the current permission owner. Current connection, child-navigation and explicit-review checks prevent stale or unintended edits. Keep the current configured-versus-active wording and running-profile eligibility. |
| `chief_plugins.rs` | Retain exact task/token and installed/excluded-plugin checks in `plugin_selection_action`. Keep `SetTaskPlugin` and the existing service/journal owner. Current explicit-review, connection and child-navigation checks protect the same operation. Subsequent-turn wording does not claim installation or current execution. |
| `chief_model_settings_wire_tests.rs` | All inherited tests remain; four accesses moved from one state field to the per-work observation map. Current additional tests cover a new explicit model's own effort choices, worker inspection versus composer ownership, and disconnect invalidation. |

| `chief_app_settings_wire_tests.rs` | The original pending-request fixture and every action/readback assertion remain, with action checks extracted into a helper. The current fixture also covers saved-connection inheritance restoration without a pending request and rejects post-write readback as new consent. |

The current permission and plugin wire tests each cover explicit one-time clicks,
lost replies, task/source changes, child navigation and connected running-task
eligibility. The model-settings module retains all three inherited tests and adds
three ownership regressions. The two app-setting wire tests also pass. All sixteen tests passed in the recent full desktop
run (519 passed, 5 ignored). These four files and their test owners are unchanged
from that tested source; byte equality was checked. No redundant code or test
implementation is introduced.

Native permission/plugin controller qualification is recorded separately in
`native-permission-controller-qualification.md` and
`native-plugin-controller-qualification.md`. Those records support the retained
eligibility semantics but do not replace signed desktop acceptance.

Close these four file dispositions only. Other shared files, historical review
notes and final signed desktop acceptance remain open. Permission and plugin
controls remain optional in the user's removal inventory; exact review identity,
unknown-result handling and no replay are required when retaining them. The model
observation tests protect the existing composer. Automations remain paused.
