> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore model-review draft coverage

The inherited desktop test `model_review_clicks_preserve_later_composer_draft`
and its two conversation fixture helpers were removed. Restore them and the
existing recovery button's debug selector. Adapt synchronization calls to the
current context parameter. Mark the fixture as controller-only so the shared
ordinary draft writer does not replace its synthetic editor state. Existing
shared-writer tests remain separate.

The restored rendered test queries the original saved request, selects an
advertised service tier, explicitly confirms the model settings, accepts the
result, and verifies that later unsent input remains unchanged. Discovery must
not dispatch a command. The accepted result must not masquerade as a new message
submission or clear the later draft.

The test passes on the current implementation. This is restored coverage, not a
claim of a reproduced production defect. The focused test passes, and all 362
Shell tests pass with one existing opt-in test ignored. Strict stable desktop
Clippy passes for all features and targets. Logs:

- `/tmp/decodex-model-review-draft-tests.log`
- `/tmp/decodex-shell-reconciliation-tests.log`
- `/tmp/decodex-model-review-draft-clippy.log`

The complete 2,156-line shell comparison has been read. Its source was unchanged
from the comparison base before this restoration. The subsequent [shell comparison](shell-owner-reconciliation.md) records the
account-control and settings-owner dispositions. The subsequent [controller comparison](conversation-controller-reconciliation.md)
closes the conversation source review. Neither a GPUI test window nor the
restored debug selector establishes signed desktop acceptance.

Model-review confirmation and preserving unsent input are core behavior for the
existing consumer. The optional History surface remains a separate product
choice. Automations remain paused.
