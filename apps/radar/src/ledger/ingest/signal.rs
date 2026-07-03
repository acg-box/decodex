use crate::{
	ledger::{
		self, ArtifactLinkInput, Connection, Path, RadarSubject, ReviewInput, eyre, ingest::bundle,
	},
	prelude::Result,
};

pub(crate) fn record_signal_artifact(
	connection: &Connection,
	signal_path: &Path,
) -> Result<Vec<RadarSubject>> {
	let signal = ledger::load_json(signal_path)?;
	let validation = ledger::validate_artifact(&signal);

	if validation.schema.as_deref() != Some(bundle::signal_schema())
		|| !validation.errors.is_empty()
	{
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(bundle::signal_schema()) {
			errors.insert(0, format!("schema must be {}", bundle::signal_schema()));
		}

		eyre::bail!(
			"Signal validation failed for {}:\n- {}",
			signal_path.display(),
			errors.join("\n- ")
		);
	}

	let signal = ledger::object_value(&signal, "signal")?;
	let slug = ledger::required_string(signal, "slug", "slug")?;
	let confidence = ledger::required_string(signal, "confidence", "confidence")?;
	let subjects = ledger::subject_refs_for_signal(signal);

	for subject in &subjects {
		ledger::record_review(
			connection,
			ReviewInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				status: "signal",
				reason: &format!("Published signal_entry/v1: {slug}"),
				confidence: Some(confidence),
			},
		)?;
		ledger::record_artifact(
			connection,
			ArtifactLinkInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				artifact_kind: "signal",
				path: signal_path,
			},
		)?;
	}

	Ok(subjects)
}
