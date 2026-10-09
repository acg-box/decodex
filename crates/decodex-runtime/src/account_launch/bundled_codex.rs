//! Protocol evidence generated from the pinned runtime during app packaging.
use super::{
	AppServerCommand, BuildId, GeneratedSchemaEvidence, ProbeError, SupervisionError,
	executable_discovery, hex_digest,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
struct RuntimeLock {
	version: String,
	targets: BTreeMap<String, Target>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
	executable_sha256: String,
	schema_sha256: String,
}
const LOCK: &str = include_str!("../../../../codex-runtime.lock.json");

pub(super) fn evidence(
	command: &AppServerCommand,
) -> Result<Option<(BuildId, GeneratedSchemaEvidence)>, ProbeError> {
	let Some(contents) = executable_discovery::bundled_contents() else {
		return Ok(None);
	};
	let lock: RuntimeLock =
		serde_json::from_str(LOCK).map_err(|_| SupervisionError::PreflightFailed)?;
	let target = lock
		.targets
		.get("aarch64-apple-darwin")
		.ok_or(SupervisionError::LaunchCapabilityUnavailable)?;
	let expected = contents.join("Resources/CodexRuntime/CodexCLI.app/Contents/MacOS/codex");
	validate_image(&command.program, &expected, &hex_digest(&command.executable_digest), target)?;
	let generated = GeneratedSchemaEvidence::load(&contents.join("Resources/CodexSchema"))
		.map_err(|markers| ProbeError::SchemaMissing { markers })?;
	if generated.fingerprint != target.schema_sha256 {
		return Err(SupervisionError::PreflightFailed.into());
	}
	let build = BuildId::from_attestation(
		&format!("codex-cli {}", lock.version),
		&command.executable_digest,
	)
	.map_err(|_| SupervisionError::PreflightFailed)?;
	Ok(Some((build, generated)))
}

fn validate_image(
	program: &Path,
	expected: &Path,
	digest: &str,
	target: &Target,
) -> Result<(), SupervisionError> {
	if program != expected.canonicalize().map_err(|_| SupervisionError::ExecutableUnavailable)?
		|| digest != target.executable_sha256
	{
		return Err(SupervisionError::ExecutableChanged);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn bundled_identity_rejects_another_binary_or_digest() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("codex");
		std::fs::write(&path, b"fixture").unwrap();
		let canonical = path.canonicalize().unwrap();
		let target =
			Target { executable_sha256: "expected".into(), schema_sha256: "schema".into() };
		assert!(validate_image(&canonical, &path, "expected", &target).is_ok());
		assert!(validate_image(&canonical, &path, "changed", &target).is_err());
		assert!(validate_image(Path::new("/another/codex"), &path, "expected", &target).is_err());
	}
}
