use std::path::PathBuf;

use super::LaneId;

/// Canonical lifecycle phase persisted by the lane aggregate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanePhase {
	Unclaimed,
	Claimed,
	Running,
	WaitingReview,
	Landed,
	Canceled,
	NeedsAttention,
}
impl LanePhase {
	pub const fn is_terminal(self) -> bool {
		matches!(self, Self::Landed | Self::Canceled | Self::NeedsAttention)
	}

	pub const fn holds_active_authority(self) -> bool {
		matches!(self, Self::Claimed | Self::Running | Self::WaitingReview)
	}

	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Unclaimed => "unclaimed",
			Self::Claimed => "claimed",
			Self::Running => "running",
			Self::WaitingReview => "waiting_review",
			Self::Landed => "landed",
			Self::Canceled => "canceled",
			Self::NeedsAttention => "needs_attention",
		}
	}

	pub(crate) fn from_str(value: &str) -> Option<Self> {
		match value {
			"unclaimed" => Some(Self::Unclaimed),
			"claimed" => Some(Self::Claimed),
			"running" => Some(Self::Running),
			"waiting_review" => Some(Self::WaitingReview),
			"landed" => Some(Self::Landed),
			"canceled" => Some(Self::Canceled),
			"needs_attention" => Some(Self::NeedsAttention),
			_ => None,
		}
	}
}

/// Sole local ownership and lifecycle projection for one lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaneAggregate {
	id: LaneId,
	binding_fingerprint: String,
	epoch: u64,
	phase: LanePhase,
	intake_authority_id: Option<String>,
	claim_run_id: Option<String>,
	branch_name: Option<String>,
	worktree_path: Option<PathBuf>,
}
impl LaneAggregate {
	pub fn new(id: LaneId, binding_fingerprint: &str) -> Self {
		Self {
			id,
			binding_fingerprint: binding_fingerprint.to_owned(),
			epoch: 0,
			phase: LanePhase::Unclaimed,
			intake_authority_id: None,
			claim_run_id: None,
			branch_name: None,
			worktree_path: None,
		}
	}

	pub(crate) fn from_persisted_parts(
		id: LaneId,
		binding_fingerprint: String,
		epoch: u64,
		phase: LanePhase,
		intake_authority_id: Option<String>,
		claim_run_id: Option<String>,
		branch_name: Option<String>,
		worktree_path: Option<PathBuf>,
	) -> Self {
		Self {
			id,
			binding_fingerprint,
			epoch,
			phase,
			intake_authority_id,
			claim_run_id,
			branch_name,
			worktree_path,
		}
	}

	pub fn id(&self) -> &LaneId {
		&self.id
	}

	pub fn binding_fingerprint(&self) -> &str {
		&self.binding_fingerprint
	}

	pub const fn epoch(&self) -> u64 {
		self.epoch
	}

	pub const fn phase(&self) -> LanePhase {
		self.phase
	}

	pub fn intake_authority_id(&self) -> Option<&str> {
		self.intake_authority_id.as_deref()
	}

	pub fn claim_run_id(&self) -> Option<&str> {
		self.claim_run_id.as_deref()
	}

	pub fn branch_name(&self) -> Option<&str> {
		self.branch_name.as_deref()
	}

	pub fn worktree_path(&self) -> Option<&PathBuf> {
		self.worktree_path.as_ref()
	}
}

/// Typed transition request. Callers cannot directly choose an output phase.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaneCommand {
	Admit { intake_authority_id: String },
	AcquireClaim { run_id: String },
	ReleaseClaim { run_id: String },
	AttachWorktree { branch_name: String, worktree_path: PathBuf },
	DetachWorktree { branch_name: String, worktree_path: PathBuf },
	BeginRun,
	BeginReview,
	Land,
	Cancel,
	RequireAttention,
}

/// Fail-closed transition rejection suitable for durable reason telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneTransitionRejection {
	BindingMismatch,
	EpochMismatch,
	InvalidPhase,
	ConflictingClaim,
	ConflictingWorktree,
	ConflictingIntakeAuthority,
	TrackerIssueAlreadyActive,
}

/// Apply one deterministic lane transition under binding and epoch CAS.
pub fn transition(
	current: &LaneAggregate,
	expected_epoch: u64,
	binding_fingerprint: &str,
	command: LaneCommand,
) -> Result<LaneAggregate, LaneTransitionRejection> {
	if current.binding_fingerprint != binding_fingerprint {
		return Err(LaneTransitionRejection::BindingMismatch);
	}
	if current.epoch != expected_epoch {
		return Err(LaneTransitionRejection::EpochMismatch);
	}
	if current.phase.is_terminal() {
		return Err(LaneTransitionRejection::InvalidPhase);
	}

	let mut next = current.clone();
	match command {
		LaneCommand::Admit { intake_authority_id } => {
			if intake_authority_id.trim().is_empty() {
				return Err(LaneTransitionRejection::ConflictingIntakeAuthority);
			}
			match current.intake_authority_id.as_deref() {
				None if current.phase == LanePhase::Unclaimed => {
					next.intake_authority_id = Some(intake_authority_id);
				},
				Some(existing) if existing == intake_authority_id => {},
				_ => return Err(LaneTransitionRejection::ConflictingIntakeAuthority),
			}
		},
		LaneCommand::AcquireClaim { run_id } => match current.claim_run_id.as_deref() {
			None if current.phase == LanePhase::Unclaimed
				&& current.intake_authority_id.is_some() =>
			{
				next.claim_run_id = Some(run_id);
				next.phase = LanePhase::Claimed;
			},
			Some(existing) if existing == run_id => {},
			Some(_) | None => return Err(LaneTransitionRejection::ConflictingClaim),
		},
		LaneCommand::ReleaseClaim { run_id } => match current.claim_run_id.as_deref() {
			Some(existing)
				if existing == run_id
					&& matches!(current.phase, LanePhase::Claimed | LanePhase::Running) =>
			{
				next.claim_run_id = None;
				next.phase = LanePhase::Unclaimed;
			},
			None if current.phase == LanePhase::Unclaimed => {},
			_ => return Err(LaneTransitionRejection::ConflictingClaim),
		},
		LaneCommand::AttachWorktree { branch_name, worktree_path } => {
			if !matches!(current.phase, LanePhase::Claimed | LanePhase::Running) {
				return Err(LaneTransitionRejection::InvalidPhase);
			}
			match (current.branch_name.as_deref(), current.worktree_path.as_ref()) {
				(None, None) => {
					next.branch_name = Some(branch_name);
					next.worktree_path = Some(worktree_path);
				},
				(Some(existing_branch), Some(existing_path))
					if existing_branch == branch_name && existing_path == &worktree_path => {},
				_ => return Err(LaneTransitionRejection::ConflictingWorktree),
			}
		},
		LaneCommand::DetachWorktree { branch_name, worktree_path } => {
			if !matches!(current.phase, LanePhase::Claimed | LanePhase::Running) {
				return Err(LaneTransitionRejection::InvalidPhase);
			}
			match (current.branch_name.as_deref(), current.worktree_path.as_ref()) {
				(Some(existing_branch), Some(existing_path))
					if existing_branch == branch_name && existing_path == &worktree_path =>
				{
					next.branch_name = None;
					next.worktree_path = None;
				},
				_ => return Err(LaneTransitionRejection::ConflictingWorktree),
			}
		},
		LaneCommand::BeginRun if current.phase == LanePhase::Claimed => {
			next.phase = LanePhase::Running;
		},
		LaneCommand::BeginRun if current.phase == LanePhase::Running => {},
		LaneCommand::BeginReview if current.phase == LanePhase::Running => {
			next.phase = LanePhase::WaitingReview;
		},
		LaneCommand::BeginReview if current.phase == LanePhase::WaitingReview => {},
		LaneCommand::Land if current.phase == LanePhase::WaitingReview => {
			next.phase = LanePhase::Landed;
		},
		LaneCommand::Cancel => next.phase = LanePhase::Canceled,
		LaneCommand::RequireAttention => next.phase = LanePhase::NeedsAttention,
		LaneCommand::BeginRun | LaneCommand::BeginReview | LaneCommand::Land => {
			return Err(LaneTransitionRejection::InvalidPhase);
		},
	}

	if next != *current {
		next.epoch += 1;
	}
	Ok(next)
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use super::{LaneAggregate, LaneCommand, LanePhase, LaneTransitionRejection, transition};
	use crate::lane_authority::LaneId;

	fn lane() -> LaneAggregate {
		LaneAggregate::from_persisted_parts(
			LaneId::new("pubfi", "issue-1").expect("lane"),
			String::from("binding-1"),
			0,
			LanePhase::Unclaimed,
			Some(String::from("authority-1")),
			None,
			None,
			None,
		)
	}

	#[test]
	fn transition_requires_fresh_binding_and_epoch() {
		let current = lane();
		assert_eq!(
			transition(
				&current,
				0,
				"wrong",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			),
			Err(LaneTransitionRejection::BindingMismatch),
		);
		assert_eq!(
			transition(
				&current,
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			),
			Err(LaneTransitionRejection::EpochMismatch),
		);
	}

	#[test]
	fn admission_is_immutable_and_retry_safe() {
		let unadmitted =
			LaneAggregate::new(LaneId::new("pubfi", "issue-1").expect("lane"), "binding-1");
		let admitted = transition(
			&unadmitted,
			0,
			"binding-1",
			LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
		)
		.expect("admit");
		assert_eq!(admitted.intake_authority_id(), Some("authority-1"));
		assert_eq!(
			transition(
				&admitted,
				1,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("replay"),
			admitted,
		);
		assert_eq!(
			transition(
				&admitted,
				1,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-2") },
			),
			Err(LaneTransitionRejection::ConflictingIntakeAuthority),
		);
	}

	#[test]
	fn replay_is_idempotent_but_conflicting_claim_is_rejected() {
		let claimed = transition(
			&lane(),
			0,
			"binding-1",
			LaneCommand::AcquireClaim { run_id: String::from("run-1") },
		)
		.expect("claim");
		let replayed = transition(
			&claimed,
			1,
			"binding-1",
			LaneCommand::AcquireClaim { run_id: String::from("run-1") },
		)
		.expect("replay");
		assert_eq!(replayed, claimed);
		assert_eq!(
			transition(
				&claimed,
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-2") },
			),
			Err(LaneTransitionRejection::ConflictingClaim),
		);
	}

	#[test]
	fn lifecycle_and_worktree_advance_only_through_typed_commands() {
		let claimed = transition(
			&lane(),
			0,
			"binding-1",
			LaneCommand::AcquireClaim { run_id: String::from("run-1") },
		)
		.expect("claim");
		let attached = transition(
			&claimed,
			1,
			"binding-1",
			LaneCommand::AttachWorktree {
				branch_name: String::from("x/issue-1"),
				worktree_path: PathBuf::from(".worktrees/issue-1"),
			},
		)
		.expect("worktree");
		let running =
			transition(&attached, 2, "binding-1", LaneCommand::BeginRun).expect("running");
		assert_eq!(
			transition(&running, 3, "binding-1", LaneCommand::BeginRun).expect("running replay"),
			running,
		);
		let review =
			transition(&running, 3, "binding-1", LaneCommand::BeginReview).expect("review");
		assert_eq!(
			transition(&review, 4, "binding-1", LaneCommand::BeginReview).expect("review replay"),
			review,
		);
		let landed = transition(&review, 4, "binding-1", LaneCommand::Land).expect("land");
		assert_eq!(landed.phase(), LanePhase::Landed);
		assert_eq!(landed.epoch(), 5);
		assert_eq!(
			transition(&landed, 5, "binding-1", LaneCommand::Cancel),
			Err(LaneTransitionRejection::InvalidPhase),
		);
	}

	#[test]
	fn claim_release_is_retry_safe_and_preserves_lane_identity() {
		let claimed = transition(
			&lane(),
			0,
			"binding-1",
			LaneCommand::AcquireClaim { run_id: String::from("run-1") },
		)
		.expect("claim");
		let released = transition(
			&claimed,
			1,
			"binding-1",
			LaneCommand::ReleaseClaim { run_id: String::from("run-1") },
		)
		.expect("release");
		assert_eq!(released.phase(), LanePhase::Unclaimed);
		assert_eq!(released.claim_run_id(), None);
		assert_eq!(
			transition(
				&released,
				2,
				"binding-1",
				LaneCommand::ReleaseClaim { run_id: String::from("run-1") },
			)
			.expect("release replay"),
			released,
		);
	}
}
