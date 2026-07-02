pub(in crate::agent::app_server) const APP_SERVER_SCHEMA_REQUIRED_MARKERS: &[&str] = &[
	"initialize",
	"config/read",
	"model/list",
	"modelProvider/capabilities/read",
	"skills/list",
	"plugin/list",
	"mcpServerStatus/list",
	"thread/start",
	"thread/resume",
	"thread/goal/set",
	"thread/goal/get",
	"thread/goal/clear",
	"thread/goal/updated",
	"turn/start",
	"thread/archive",
	"command/exec",
	"item/tool/call",
	"thread/status/changed",
	"turn/completed",
	"dynamicTools",
	"function",
	"namespace",
	"tools",
	"type",
	"deferLoading",
	"inputText",
	"marketplaceKinds",
];
pub(in crate::agent::app_server) const APP_SERVER_REQUIRED_CLIENT_REQUESTS: &[(&str, &str)] = &[
	("initialize", "InitializeParams"),
	("account/login/start", "LoginAccountParams"),
	("thread/start", "ThreadStartParams"),
	("thread/resume", "ThreadResumeParams"),
	("thread/archive", "ThreadArchiveParams"),
	("thread/goal/set", "ThreadGoalSetParams"),
	("thread/goal/get", "ThreadGoalGetParams"),
	("thread/goal/clear", "ThreadGoalClearParams"),
	("turn/start", "TurnStartParams"),
	("turn/interrupt", "TurnInterruptParams"),
	("turn/steer", "TurnSteerParams"),
	("command/exec", "CommandExecParams"),
	("config/read", "ConfigReadParams"),
	("model/list", "ModelListParams"),
	("modelProvider/capabilities/read", "ModelProviderCapabilitiesReadParams"),
	("skills/list", "SkillsListParams"),
	("plugin/list", "PluginListParams"),
	("mcpServerStatus/list", "ListMcpServerStatusParams"),
];
pub(in crate::agent::app_server) const APP_SERVER_REQUIRED_SERVER_REQUESTS: &[(&str, &str)] = &[
	("item/commandExecution/requestApproval", "CommandExecutionRequestApprovalParams"),
	("item/fileChange/requestApproval", "FileChangeRequestApprovalParams"),
	("item/tool/requestUserInput", "ToolRequestUserInputParams"),
	("mcpServer/elicitation/request", "McpServerElicitationRequestParams"),
	("item/permissions/requestApproval", "PermissionsRequestApprovalParams"),
	("item/tool/call", "DynamicToolCallParams"),
	("account/chatgptAuthTokens/refresh", "ChatgptAuthTokensRefreshParams"),
];
pub(in crate::agent::app_server) const APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS: &[(&str, &str)] =
	&[("initialized", "")];
pub(in crate::agent::app_server) const APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS: &[(&str, &str)] =
	&[
		("error", "ErrorNotification"),
		("thread/started", "ThreadStartedNotification"),
		("thread/status/changed", "ThreadStatusChangedNotification"),
		("thread/archived", "ThreadArchivedNotification"),
		("thread/goal/updated", "ThreadGoalUpdatedNotification"),
		("thread/goal/cleared", "ThreadGoalClearedNotification"),
		("thread/tokenUsage/updated", "ThreadTokenUsageUpdatedNotification"),
		("turn/started", "TurnStartedNotification"),
		("turn/completed", "TurnCompletedNotification"),
		("item/started", "ItemStartedNotification"),
		("item/completed", "ItemCompletedNotification"),
		("item/agentMessage/delta", "AgentMessageDeltaNotification"),
		("account/updated", "AccountUpdatedNotification"),
		("account/rateLimits/updated", "AccountRateLimitsUpdatedNotification"),
		("model/rerouted", "ModelReroutedNotification"),
		("model/verification", "ModelVerificationNotification"),
	];

pub(in crate::agent::app_server::schema_probe) const APP_SERVER_SCHEMA_GENERATE_COMMAND: &str =
	"codex app-server generate-json-schema --experimental";
pub(in crate::agent::app_server::schema_probe) const APP_SERVER_SCHEMA_PROBE_OUT_DIR: &str =
	"target/decodex-app-server-schema-check";
pub(in crate::agent::app_server::schema_probe) const APP_SERVER_SCHEMA_PROSE_KEYS: &[&str] =
	&["$comment", "comment", "description", "examples", "markdownDescription", "title"];
