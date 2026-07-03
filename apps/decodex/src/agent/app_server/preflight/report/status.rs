use crate::agent::app_server::preflight::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AppServerCapabilityPreflightStatus {
	Ok,
	Blocked,
}
