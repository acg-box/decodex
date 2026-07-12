use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	lane_authority::{
		EffectClass, EffectCommand, EffectReceipt, EffectState, LaneEffect, LaneEffectKind,
	},
	prelude::{Result, eyre},
	recovery::{AdoptValidation, LinearExecutionEventRecord, RecoveryContext},
};

pub(super) fn plan_adopt_audit_effect(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	event: &LinearExecutionEventRecord,
) -> Result<LaneEffect> {
	let project_id = context.config.service_id();
	let lane_id = crate::lane_authority::LaneId::new(project_id, &validation.issue.id)?;
	let lane = context
		.state_store
		.lane(&lane_id)?
		.ok_or_else(|| eyre::eyre!("Recovery adoption audit requires a canonical lane."))?;
	let request_digest = digest(&serde_json::to_vec(event)?);
	let desired_state_digest = digest(event.idempotency_key.as_bytes());
	let facts_fingerprint = digest_parts(&[
		&validation.issue.updated_at,
		&validation.local_head_oid,
		lane.binding_fingerprint(),
		&lane.epoch().to_string(),
	]);
	let operation_id = format!("recovery-adopt:{}", validation.run_id);
	LaneEffect::plan(
		&format!("{operation_id}:linear-audit"),
		&operation_id,
		0,
		lane_id,
		lane.binding_fingerprint(),
		&validation.run_id,
		lane.epoch(),
		LaneEffectKind::LinearIssueCommentCreate,
		EffectClass::DurablePublication,
		&event.idempotency_key,
		&request_digest,
		&desired_state_digest,
		&facts_fingerprint,
	)
}

pub(super) fn begin_adopt_audit_effect(
	context: &RecoveryContext,
	effect: &LaneEffect,
) -> Result<LaneEffect> {
	let effect = context.state_store.plan_lane_effect(effect.clone())?;
	let effect = context
		.state_store
		.lane_effect(effect.effect_id())?
		.ok_or_else(|| eyre::eyre!("Planned recovery adoption effect disappeared."))?;
	match effect.state() {
		EffectState::Planned => context.state_store.apply_lane_effect_command(
			effect.effect_id(),
			effect.journal_epoch(),
			EffectCommand::BeginInvocation {
				authority_epoch: effect.authority_epoch(),
				facts_fingerprint: effect.facts_fingerprint().to_owned(),
			},
		),
		EffectState::Invoking => context.state_store.apply_lane_effect_command(
			effect.effect_id(),
			effect.journal_epoch(),
			EffectCommand::MarkOutcomeUnknown,
		),
		EffectState::ReconciliationRequired | EffectState::Succeeded => Ok(effect),
		_ => eyre::bail!("Recovery adoption audit effect cannot resume from its current state."),
	}
}

pub(super) fn mark_adopt_audit_outcome_unknown(
	context: &RecoveryContext,
	effect: &LaneEffect,
) -> Result<()> {
	if effect.state() == EffectState::ReconciliationRequired {
		return Ok(());
	}
	context
		.state_store
		.apply_lane_effect_command(
			effect.effect_id(),
			effect.journal_epoch(),
			EffectCommand::MarkOutcomeUnknown,
		)
		.map(|_| ())
}

pub(super) fn record_adopt_audit_receipt(
	context: &RecoveryContext,
	effect: &LaneEffect,
	created: bool,
) -> Result<()> {
	let now = OffsetDateTime::now_utc();
	let observed_at = now.format(&Rfc3339)?;
	let receipt = EffectReceipt::new(
		&format!("{}:receipt", effect.effect_id()),
		effect.request_digest(),
		if created { "sha256:created" } else { "sha256:already-present" },
		None,
		None,
		&observed_at,
		now.unix_timestamp(),
	)?;
	context
		.state_store
		.apply_lane_effect_command(
			effect.effect_id(),
			effect.journal_epoch(),
			EffectCommand::RecordReceipt { receipt },
		)
		.map(|_| ())
}

fn digest(bytes: &[u8]) -> String {
	let digest = Sha256::digest(bytes);
	format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

fn digest_parts(parts: &[&str]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update((part.len() as u64).to_be_bytes());
		digest.update(part.as_bytes());
	}
	format!(
		"sha256:{}",
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	)
}
