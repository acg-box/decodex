mod event;
mod matching;
mod pr_status;

pub(crate) use self::event::operator_autonomy_replay_evidence_status_from_event;

pub(crate) fn operator_autonomy_evidence_completeness_rank(value: &str) -> u8 {
	match value {
		"complete" => 1,
		_ => 0,
	}
}
