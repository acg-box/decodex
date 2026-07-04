use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct CodexFastModeResponse {
	pub(crate) codex_config_path: String,
	pub(crate) enabled: bool,
}
