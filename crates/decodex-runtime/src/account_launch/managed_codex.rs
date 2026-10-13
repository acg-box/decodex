//! Attest the exact component selected by this Decodex build's runtime lock.
use super::{AppServerCommand, BuildId, GeneratedSchemaEvidence, ProbeError, SupervisionError};
use decodex_components::{ComponentStore, RuntimeLock};

pub(super) fn evidence(
	command: &AppServerCommand,
) -> Result<Option<(BuildId, GeneratedSchemaEvidence)>, ProbeError> {
	let lock = RuntimeLock::embedded().map_err(|_| SupervisionError::PreflightFailed)?;
	let store = ComponentStore::for_user().map_err(|_| SupervisionError::ExecutableUnavailable)?;
	let Ok(paths) = store.paths(&lock) else {
		return Ok(None);
	};
	// Explicit native acceptance fixtures retain their own executable and schema probes.
	if paths.executable.canonicalize().ok().as_ref() != Some(&command.program) {
		return Ok(None);
	}
	let runtime = store
		.installed(&lock)
		.map_err(|_| SupervisionError::ExecutableChanged)?
		.ok_or(SupervisionError::ExecutableUnavailable)?;
	let generated = GeneratedSchemaEvidence::load(&runtime.schemas)
		.map_err(|markers| ProbeError::SchemaMissing { markers })?;
	if generated.fingerprint
		!= lock.target().map_err(|_| SupervisionError::PreflightFailed)?.1.schema_sha256
	{
		return Err(SupervisionError::PreflightFailed.into());
	}
	let build = BuildId::from_attestation(
		&format!("codex-cli {}", lock.version),
		&command.executable_digest,
	)
	.map_err(|_| SupervisionError::PreflightFailed)?;
	Ok(Some((build, generated)))
}
