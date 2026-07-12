use serde::{Deserialize, Serialize};

use super::LaneId;
use crate::prelude::{Result, eyre};

pub const LANE_EFFECT_SCHEMA: &str = "decodex/lane-effect/1";
pub const EFFECT_RECEIPT_SCHEMA: &str = "decodex/effect-receipt/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
	Compensable,
	DurablePublication,
	IrreversibleTerminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneEffectKind {
	LinearIssueCommentCreate,
	LinearIssueLabelAdd,
	LinearIssueLabelRemove,
	LinearIssueStateSet,
	GithubPrCommentCreate,
	GithubPrMerge,
	ProcessThreadArchive,
}
impl LaneEffectKind {
	pub const fn registry_name(self) -> &'static str {
		match self {
			Self::LinearIssueCommentCreate => "linear.issue.comment_create",
			Self::LinearIssueLabelAdd => "linear.issue.label_add",
			Self::LinearIssueLabelRemove => "linear.issue.label_remove",
			Self::LinearIssueStateSet => "linear.issue.state_set",
			Self::GithubPrCommentCreate => "github.pr.comment_create",
			Self::GithubPrMerge => "github.pr.merge",
			Self::ProcessThreadArchive => "process.thread.archive",
		}
	}

	pub const fn required_class(self) -> EffectClass {
		match self {
			Self::LinearIssueCommentCreate
			| Self::GithubPrCommentCreate
			| Self::ProcessThreadArchive => EffectClass::DurablePublication,
			Self::LinearIssueLabelAdd
			| Self::LinearIssueLabelRemove
			| Self::LinearIssueStateSet => EffectClass::Compensable,
			Self::GithubPrMerge => EffectClass::IrreversibleTerminal,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
	Planned,
	Invoking,
	ReconciliationRequired,
	Succeeded,
	Compensating,
	Compensated,
	NeedsAttention,
}
impl EffectState {
	pub const fn is_terminal(self) -> bool {
		matches!(self, Self::Succeeded | Self::Compensated | Self::NeedsAttention)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EffectReceipt {
	schema: String,
	receipt_id: String,
	request_digest: String,
	result_digest: String,
	provider_object_id: Option<String>,
	provider_version: Option<String>,
	observed_at: String,
	observed_at_unix: i64,
}
impl EffectReceipt {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		receipt_id: &str,
		request_digest: &str,
		result_digest: &str,
		provider_object_id: Option<&str>,
		provider_version: Option<&str>,
		observed_at: &str,
		observed_at_unix: i64,
	) -> Result<Self> {
		for (field, value) in [
			("receipt_id", receipt_id),
			("request_digest", request_digest),
			("result_digest", result_digest),
			("observed_at", observed_at),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Effect receipt `{field}` cannot be empty.");
			}
		}
		Ok(Self {
			schema: String::from(EFFECT_RECEIPT_SCHEMA),
			receipt_id: receipt_id.to_owned(),
			request_digest: request_digest.to_owned(),
			result_digest: result_digest.to_owned(),
			provider_object_id: provider_object_id.map(ToOwned::to_owned),
			provider_version: provider_version.map(ToOwned::to_owned),
			observed_at: observed_at.to_owned(),
			observed_at_unix,
		})
	}

	pub fn request_digest(&self) -> &str {
		&self.request_digest
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct LaneEffect {
	schema: String,
	effect_id: String,
	operation_id: String,
	ordinal: u32,
	lane_id: LaneId,
	binding_fingerprint: String,
	claim_run_id: String,
	expected_lane_epoch: u64,
	kind: LaneEffectKind,
	class: EffectClass,
	idempotency_key: String,
	request_digest: String,
	desired_state_digest: String,
	facts_fingerprint: String,
	state: EffectState,
	journal_epoch: u64,
	receipt: Option<EffectReceipt>,
}
impl LaneEffect {
	#[allow(clippy::too_many_arguments)]
	pub fn plan(
		effect_id: &str,
		operation_id: &str,
		ordinal: u32,
		lane_id: LaneId,
		binding_fingerprint: &str,
		claim_run_id: &str,
		expected_lane_epoch: u64,
		kind: LaneEffectKind,
		class: EffectClass,
		idempotency_key: &str,
		request_digest: &str,
		desired_state_digest: &str,
		facts_fingerprint: &str,
	) -> Result<Self> {
		for (field, value) in [
			("effect_id", effect_id),
			("operation_id", operation_id),
			("binding_fingerprint", binding_fingerprint),
			("claim_run_id", claim_run_id),
			("idempotency_key", idempotency_key),
			("request_digest", request_digest),
			("desired_state_digest", desired_state_digest),
			("facts_fingerprint", facts_fingerprint),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Lane effect `{field}` cannot be empty.");
			}
		}
		if class != kind.required_class() {
			eyre::bail!("Lane effect class does not match the normative effect registry.");
		}
		Ok(Self {
			schema: String::from(LANE_EFFECT_SCHEMA),
			effect_id: effect_id.to_owned(),
			operation_id: operation_id.to_owned(),
			ordinal,
			lane_id,
			binding_fingerprint: binding_fingerprint.to_owned(),
			claim_run_id: claim_run_id.to_owned(),
			expected_lane_epoch,
			kind,
			class,
			idempotency_key: idempotency_key.to_owned(),
			request_digest: request_digest.to_owned(),
			desired_state_digest: desired_state_digest.to_owned(),
			facts_fingerprint: facts_fingerprint.to_owned(),
			state: EffectState::Planned,
			journal_epoch: 0,
			receipt: None,
		})
	}

	pub fn effect_id(&self) -> &str {
		&self.effect_id
	}

	pub fn operation_id(&self) -> &str {
		&self.operation_id
	}

	pub const fn ordinal(&self) -> u32 {
		self.ordinal
	}

	pub const fn kind(&self) -> LaneEffectKind {
		self.kind
	}

	pub fn lane_id(&self) -> &LaneId {
		&self.lane_id
	}

	pub const fn state(&self) -> EffectState {
		self.state
	}

	pub const fn journal_epoch(&self) -> u64 {
		self.journal_epoch
	}

	pub const fn expected_lane_epoch(&self) -> u64 {
		self.expected_lane_epoch
	}

	pub fn claim_run_id(&self) -> &str {
		&self.claim_run_id
	}

	pub fn binding_fingerprint(&self) -> &str {
		&self.binding_fingerprint
	}

	pub fn request_digest(&self) -> &str {
		&self.request_digest
	}

	pub fn facts_fingerprint(&self) -> &str {
		&self.facts_fingerprint
	}

	pub fn receipt(&self) -> Option<&EffectReceipt> {
		self.receipt.as_ref()
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != LANE_EFFECT_SCHEMA || self.class != self.kind.required_class() {
			eyre::bail!("Lane effect schema or registry class is invalid.");
		}
		if let Some(receipt) = self.receipt.as_ref()
			&& (receipt.schema != EFFECT_RECEIPT_SCHEMA
				|| receipt.request_digest != self.request_digest)
		{
			eyre::bail!("Lane effect receipt is not bound to its request.");
		}
		Ok(())
	}

	pub fn has_same_plan_identity(&self, other: &Self) -> bool {
		self.schema == other.schema
			&& self.effect_id == other.effect_id
			&& self.operation_id == other.operation_id
			&& self.ordinal == other.ordinal
			&& self.lane_id == other.lane_id
			&& self.binding_fingerprint == other.binding_fingerprint
			&& self.claim_run_id == other.claim_run_id
			&& self.expected_lane_epoch == other.expected_lane_epoch
			&& self.kind == other.kind
			&& self.class == other.class
			&& self.idempotency_key == other.idempotency_key
			&& self.request_digest == other.request_digest
			&& self.desired_state_digest == other.desired_state_digest
			&& self.facts_fingerprint == other.facts_fingerprint
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectCommand {
	BeginInvocation { lane_epoch: u64, facts_fingerprint: String },
	RecordReceipt { receipt: EffectReceipt },
	MarkOutcomeUnknown,
	BeginCompensation,
	CompleteCompensation { receipt: EffectReceipt },
	RequireAttention,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneEffectRejection {
	JournalEpochMismatch,
	LaneEpochMismatch,
	FactsDrift,
	InvalidState,
	ReceiptMismatch,
	CompensationForbidden,
}

pub fn apply_effect_command(
	current: &LaneEffect,
	expected_journal_epoch: u64,
	command: EffectCommand,
) -> std::result::Result<LaneEffect, LaneEffectRejection> {
	if current.journal_epoch != expected_journal_epoch {
		return Err(LaneEffectRejection::JournalEpochMismatch);
	}
	let mut next = current.clone();
	match command {
		EffectCommand::BeginInvocation { lane_epoch, facts_fingerprint } => {
			if current.state == EffectState::Invoking
				&& lane_epoch == current.expected_lane_epoch
				&& facts_fingerprint == current.facts_fingerprint
			{
				return Ok(next);
			}
			if current.state != EffectState::Planned {
				return Err(LaneEffectRejection::InvalidState);
			}
			if lane_epoch != current.expected_lane_epoch {
				return Err(LaneEffectRejection::LaneEpochMismatch);
			}
			if facts_fingerprint != current.facts_fingerprint {
				return Err(LaneEffectRejection::FactsDrift);
			}
			next.state = EffectState::Invoking;
		},
		EffectCommand::RecordReceipt { receipt } => {
			if current.state == EffectState::Succeeded && current.receipt.as_ref() == Some(&receipt)
			{
				return Ok(next);
			}
			if !matches!(current.state, EffectState::Invoking | EffectState::ReconciliationRequired)
			{
				return Err(LaneEffectRejection::InvalidState);
			}
			if receipt.request_digest() != current.request_digest {
				return Err(LaneEffectRejection::ReceiptMismatch);
			}
			next.receipt = Some(receipt);
			next.state = EffectState::Succeeded;
		},
		EffectCommand::MarkOutcomeUnknown => {
			if current.state != EffectState::Invoking {
				return Err(LaneEffectRejection::InvalidState);
			}
			next.state = EffectState::ReconciliationRequired;
		},
		EffectCommand::BeginCompensation => {
			if current.class != EffectClass::Compensable {
				return Err(LaneEffectRejection::CompensationForbidden);
			}
			if current.state != EffectState::Succeeded {
				return Err(LaneEffectRejection::InvalidState);
			}
			next.state = EffectState::Compensating;
		},
		EffectCommand::CompleteCompensation { receipt } => {
			if current.state != EffectState::Compensating {
				return Err(LaneEffectRejection::InvalidState);
			}
			if receipt.request_digest() != current.request_digest {
				return Err(LaneEffectRejection::ReceiptMismatch);
			}
			next.receipt = Some(receipt);
			next.state = EffectState::Compensated;
		},
		EffectCommand::RequireAttention => {
			if current.state.is_terminal() {
				return Err(LaneEffectRejection::InvalidState);
			}
			next.state = EffectState::NeedsAttention;
		},
	}
	if next != *current {
		next.journal_epoch = current.journal_epoch.saturating_add(1);
	}
	Ok(next)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn effect(kind: LaneEffectKind) -> LaneEffect {
		LaneEffect::plan(
			"effect-1",
			"operation-1",
			0,
			LaneId::new("pubfi", "PUB-101").expect("lane"),
			"binding-1",
			"run-1",
			7,
			kind,
			kind.required_class(),
			"idempotency-1",
			"sha256:request",
			"sha256:desired",
			"sha256:facts",
		)
		.expect("effect")
	}

	fn receipt() -> EffectReceipt {
		EffectReceipt::new(
			"receipt-1",
			"sha256:request",
			"sha256:result",
			Some("provider-object-1"),
			Some("version-1"),
			"2026-07-12T00:00:00Z",
			1,
		)
		.expect("receipt")
	}

	#[test]
	fn invocation_requires_fresh_lane_epoch_and_facts() {
		let effect = effect(LaneEffectKind::LinearIssueCommentCreate);
		assert_eq!(
			apply_effect_command(
				&effect,
				0,
				EffectCommand::BeginInvocation {
					lane_epoch: 6,
					facts_fingerprint: String::from("sha256:facts"),
				},
			),
			Err(LaneEffectRejection::LaneEpochMismatch),
		);
		assert_eq!(
			apply_effect_command(
				&effect,
				0,
				EffectCommand::BeginInvocation {
					lane_epoch: 7,
					facts_fingerprint: String::from("sha256:stale"),
				},
			),
			Err(LaneEffectRejection::FactsDrift),
		);
	}

	#[test]
	fn unknown_outcome_reconciles_before_receipt_and_exact_replay_is_noop() {
		let planned = effect(LaneEffectKind::LinearIssueCommentCreate);
		let invoking = apply_effect_command(
			&planned,
			0,
			EffectCommand::BeginInvocation {
				lane_epoch: 7,
				facts_fingerprint: String::from("sha256:facts"),
			},
		)
		.expect("invoke");
		let unknown =
			apply_effect_command(&invoking, 1, EffectCommand::MarkOutcomeUnknown).expect("unknown");
		let succeeded =
			apply_effect_command(&unknown, 2, EffectCommand::RecordReceipt { receipt: receipt() })
				.expect("receipt");
		assert_eq!(succeeded.state(), EffectState::Succeeded);
		assert_eq!(
			apply_effect_command(
				&succeeded,
				3,
				EffectCommand::RecordReceipt { receipt: receipt() },
			)
			.expect("receipt replay"),
			succeeded,
		);
	}

	#[test]
	fn irreversible_effect_cannot_compensate() {
		let planned = effect(LaneEffectKind::GithubPrMerge);
		let invoking = apply_effect_command(
			&planned,
			0,
			EffectCommand::BeginInvocation {
				lane_epoch: 7,
				facts_fingerprint: String::from("sha256:facts"),
			},
		)
		.expect("invoke");
		let succeeded =
			apply_effect_command(&invoking, 1, EffectCommand::RecordReceipt { receipt: receipt() })
				.expect("receipt");
		assert_eq!(
			apply_effect_command(&succeeded, 2, EffectCommand::BeginCompensation),
			Err(LaneEffectRejection::CompensationForbidden),
		);
	}
}
