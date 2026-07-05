use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneHardInterruptReport {
	pub(in crate::orchestrator::lane_control) attempted: bool,
	pub(in crate::orchestrator::lane_control) status: String,
	pub(in crate::orchestrator::lane_control) classification: String,
	pub(in crate::orchestrator::lane_control) signals: Vec<String>,
	pub(in crate::orchestrator::lane_control) process_id: Option<u32>,
	pub(in crate::orchestrator::lane_control) process_alive_after: Option<bool>,
	pub(in crate::orchestrator::lane_control) message: String,
	pub(in crate::orchestrator::lane_control) error_class: Option<String>,
}
impl LaneHardInterruptReport {
	pub(in crate::orchestrator::lane_control) fn unavailable(
		error_class: &str,
		message: &str,
	) -> Self {
		Self {
			attempted: false,
			status: String::from("unavailable"),
			classification: String::from("hard_interrupt_fallback"),
			signals: Vec::new(),
			process_id: None,
			process_alive_after: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
		}
	}
}
