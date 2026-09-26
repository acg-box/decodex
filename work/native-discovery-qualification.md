# Native discovery qualification

Classification: evidence for existing native integration and policy consumers.
Restore three inherited installed-native tests in a dedicated module that uses
the existing retained bridge and process fixture. No application behavior or
configuration changes. All three pass on installed Codex 0.158.0-alpha.2.

- MCP status retains initialization capabilities when tool discovery fails.
  A failed initialization has no advertised capabilities. Three one-item pages
  reproduce the complete result, both before and after process restart.
- Permission profiles use each requested working directory. A directory with
  brackets and an ordinary directory yield their expected independent eligibility.
  Reads do not change configuration.
- Enterprise MCP registration cannot be downgraded or redirected through project
  configuration. Seven changes to authentication, URL, resource, scopes and OAuth
  identity are rejected across two native processes. Native EMA OAuth login is
  unavailable; this test does not claim working enterprise authentication.

The MCP server is a preserved 27-line Python fixture that uses stdin/stdout only.
Its bytes match the original snapshot. The tests create private temporary homes,
use synthetic configuration and reap their native child through the existing
session owner. No real credential, model request or external provider is needed.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` retains the EMA login
refusal in `app-server/src/request_processors/mcp_processor.rs` and the atomic
non-project registration check in `config/src/mcp_ema.rs`. Native Codex owns both
policies. Decodex's existing integration projection keeps `serverCapabilities`
separate from `toolsError`.

This closes the complete Python fixture disposition. The shared native test
module still has other inherited differences, in additional native fixture modules. Its ledger row remains open. The result is not signed
desktop acceptance, managed policy deployment or support for enterprise login.
Automations remain paused.

## Native media and summary history

A second focused commit restores the inherited notification-media comparison.
Run the same two image-bearing turns with native notification-media filtering off
and on. Both user item notifications retain text; their image content follows the
filter setting. The captured model requests contain the same image inputs in both
runs, and persisted history remains readable after process restart. The original
valid PNG fixture is retained. The current `serve_fixture` remains the single
backend helper; no old duplicate helper stack is restored.

Installed Codex 0.158.0-alpha.2 passes both filter cases. The fixed upstream
`turn_start_omits_notification_media_without_changing_model_input` test and
`notification_media.rs` define the same distinction between notifications and
model input. They were inspected, not run locally. Only private fixture config
sets the feature; the user's application configuration stays unchanged.

The inherited standalone summary/cold-restart test also passes. See
[history summary recovery](history-summary-recovery.md). This closes that complete
document's disposition, not the parent native-test file or final desktop review.

## Tool audio and stored image references

Restore the complete inherited audio and file-image test files, replacing only
the retired backend helper calls with current `serve_fixture` calls. Installed
Codex 0.158.0-alpha.2 passes both tests. An empty tool-audio payload becomes the
native omission placeholder while adjacent text remains in order. File and inline
images retain order and history detail through cold readback and one explicit
continuation, without duplicated images. Both notification-filter settings pass.
Model-wire detail remains absent under the native Responses Lite contract.

The fixed upstream audio preparation test preserves valid content and replaces
only failed audio. The existing app-server media test retains file and inline
images on the model wire while filtering notifications. These policies remain
native-owned; this batch adds no media conversion or feature setting to Decodex.
Both complete inherited test-file dispositions close. Live voice, provider-backed
media and final desktop acceptance remain separate.
