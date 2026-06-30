use std::time::Duration;

pub(crate) const RUN_LEASE_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MODEL_EXECUTION_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const RUN_CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub(super) const PROBE_RUN_ID: &str = "protocol-probe-run";
pub(super) const PROBE_ISSUE_ID: &str = "protocol-probe";
pub(super) const PROBE_EXPECTED_OUTPUT: &str = "PROBE_OK";
pub(super) const PROBE_COMMAND_EXEC_EXPECTED_OUTPUT: &str = "COMMAND_EXEC_OK";
pub(super) const PROBE_COMMAND_EXEC_TIMEOUT_MS: u64 = 5_000;
pub(super) const PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP: u64 = 1_024;
pub(super) const PROBE_DEVELOPER_INSTRUCTIONS: &str = "You are a protocol probe. You must call the dynamic tool `echo_probe` exactly once with the JSON argument `{\"text\":\"PROBE_OK\"}`. Do not use shell. Do not inspect files. After the tool response is returned, reply with the exact text PROBE_OK and nothing else.";
pub(super) const PROBE_USER_INPUT: &str = "Call `echo_probe` with `{\\\"text\\\":\\\"PROBE_OK\\\"}`. After the tool succeeds, reply with the exact text PROBE_OK.";
pub(super) const PREFLIGHT_EVENT_TYPE: &str = "app-server/preflight";
pub(super) const PREFLIGHT_MODEL_PAGE_LIMIT: u32 = 200;
pub(super) const PREFLIGHT_MCP_PAGE_LIMIT: u32 = 50;
pub(super) const PREFLIGHT_MCP_DETAIL: &str = "toolsAndAuthOnly";
pub(super) const PREFLIGHT_CHECK_CONFIG: &str = "config";
pub(super) const PREFLIGHT_CHECK_MODEL: &str = "model";
pub(super) const PREFLIGHT_CHECK_MODEL_PROVIDER: &str = "model_provider";
pub(super) const PREFLIGHT_CHECK_SKILLS: &str = "skills";
pub(super) const PREFLIGHT_CHECK_PLUGINS: &str = "plugins";
pub(super) const PREFLIGHT_CHECK_MCP: &str = "mcp";
pub(super) const PREFLIGHT_PLUGIN_MARKETPLACE_KIND: &str = "local";
pub(super) const JSONRPC_METHOD_NOT_FOUND: i64 = -32_601;
