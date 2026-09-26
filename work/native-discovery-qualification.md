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
module still has other inherited differences, including media notification and
summary-history coverage. Its ledger row remains open. The result is not signed
desktop acceptance, managed policy deployment or support for enterprise login.
Automations remain paused.
