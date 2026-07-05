use serde::Serialize;

use crate::{
	run_control::{LaneControlInterruptResponse, LaneControlResponseStatus},
	state::RunControlActionReceipt,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneSoftInterruptReport {
	pub(in crate::orchestrator::lane_control) attempted: bool,
	pub(in crate::orchestrator::lane_control) available: bool,
	pub(in crate::orchestrator::lane_control) status: String,
	pub(in crate::orchestrator::lane_control) classification: String,
	pub(in crate::orchestrator::lane_control) method: String,
	pub(in crate::orchestrator::lane_control) request_id: Option<String>,
	pub(in crate::orchestrator::lane_control) message: String,
	pub(in crate::orchestrator::lane_control) error_class: Option<String>,
	pub(in crate::orchestrator::lane_control) protocol_summary: Option<String>,
	pub(in crate::orchestrator::lane_control) response: Option<LaneControlInterruptResponse>,
}
impl LaneSoftInterruptReport {
	pub(in crate::orchestrator::lane_control) fn unavailable(
		error_class: &str,
		message: &str,
	) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("unavailable"),
			classification: String::from("soft_interrupt_unavailable"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: message.to_owned(),
			error_class: Some(error_class.to_owned()),
			protocol_summary: None,
			response: None,
		}
	}

	pub(in crate::orchestrator::lane_control) fn from_response(
		response: LaneControlInterruptResponse,
	) -> Self {
		let status = match &response.status {
			LaneControlResponseStatus::SoftDelivered => "delivered",
			LaneControlResponseStatus::SoftFailed => "failed",
			LaneControlResponseStatus::Rejected => "rejected",
		};

		Self {
			attempted: true,
			available: true,
			status: String::from(status),
			classification: response.classification.clone(),
			method: response.method.clone(),
			request_id: Some(response.request_id.clone()),
			message: response.message.clone(),
			error_class: response.error_class.clone(),
			protocol_summary: response.protocol_summary.clone(),
			response: Some(response),
		}
	}

	pub(in crate::orchestrator::lane_control) fn from_control_rejection(
		receipt: &RunControlActionReceipt,
	) -> Self {
		Self {
			attempted: false,
			available: false,
			status: String::from("rejected"),
			classification: String::from("control_request_rejected"),
			method: String::from("turn/interrupt"),
			request_id: None,
			message: format!(
				"Run-control resolver rejected the interrupt request: {}.",
				receipt.reason()
			),
			error_class: Some(receipt.reason().to_owned()),
			protocol_summary: None,
			response: None,
		}
	}
}
