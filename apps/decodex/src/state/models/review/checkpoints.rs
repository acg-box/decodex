/// Latest runtime-owned review-policy checkpoint for one run phase.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyCheckpoint {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) phase: String,
	pub(in crate::state) status: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) nonclean_rounds: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[cfg_attr(not(test), allow(dead_code))]
impl ReviewPolicyCheckpoint {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn status(&self) -> &str {
		&self.status
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn nonclean_rounds(&self) -> i64 {
		self.nonclean_rounds
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Latest loop-guardrail checkpoint for one issue and stop reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoopGuardrailCheckpoint {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) reason: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) consecutive_count: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl LoopGuardrailCheckpoint {
	#[cfg(test)]
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	#[cfg(test)]
	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn consecutive_count(&self) -> i64 {
		self.consecutive_count
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	#[cfg(test)]
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	#[cfg(test)]
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
