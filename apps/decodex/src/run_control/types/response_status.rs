use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaneControlResponseStatus {
	SoftDelivered,
	SoftFailed,
	Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaneControlSteerResponseStatus {
	Delivered,
	Failed,
	Rejected,
}
