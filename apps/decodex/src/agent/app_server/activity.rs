mod child;
mod payload;
mod protocol;

pub(super) use self::{
	child::ChildActivityAccumulator, payload::redact_identifier,
	protocol::accumulator::ProtocolActivityAccumulator,
};

use std::time::Duration;

use crate::state::ProtocolActivitySummary;

const CHILD_BUCKET_MODEL: &str = "Model";
const WAITING_REASON_MODEL_EXECUTION: &str = "model_execution";
const CHILD_BUCKET_PROTOCOL: &str = "Protocol";
const CHILD_BUCKET_TOOL: &str = "Tool";
const CHILD_BUCKET_SHELL: &str = "Shell";
const CHILD_BUCKET_TRACKER: &str = "Tracker";
const CHILD_BUCKET_BROWSER_IMAGE: &str = "Browser/Image";
const CHILD_BUCKET_PR_LAND: &str = "PR/Land";
const LARGE_CHILD_OUTPUT_BYTES: i64 = 100_000;
const RECENT_PROTOCOL_ACTIVITY_LIMIT: usize = 8;
const INPUT_TOKEN_KEYS: &[&str] = &[
	"input_tokens",
	"inputTokens",
	"prompt_tokens",
	"promptTokens",
	"total_input_tokens",
	"totalInputTokens",
];
const OUTPUT_TOKEN_KEYS: &[&str] = &[
	"output_tokens",
	"outputTokens",
	"completion_tokens",
	"completionTokens",
	"total_output_tokens",
	"totalOutputTokens",
];

pub(crate) fn protocol_activity_idle_timeout(
	protocol_activity: Option<&ProtocolActivitySummary>,
	base_timeout: Duration,
) -> Duration {
	protocol::protocol_activity_idle_timeout(protocol_activity, base_timeout)
}

fn duration_seconds_i64(duration: Duration) -> i64 {
	i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}
