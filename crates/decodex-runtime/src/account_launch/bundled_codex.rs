//! Protocol evidence generated from the pinned runtime during app packaging.
use super::{
	AppServerCommand, BuildId, GeneratedSchemaEvidence, ProbeError, SupervisionError,
	executable_discovery, hex_digest,
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
struct RuntimeLock {
	#[serde(rename = "upstreamCommit")]
	upstream_commit: String,
	version: String,
	targets: BTreeMap<String, Target>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
	executable_sha256: Option<String>,
	schema_sha256: String,
}
const LOCK: &str = include_str!("../../../../codex-runtime.lock.json");
const BUILT: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime-evidence.json"));

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltRuntime {
	upstream_commit: String,
	executable_sha256: String,
	schema_sha256: String,
}

fn executable_digest(
	lock: &RuntimeLock,
	target: &Target,
	built: &str,
) -> Result<String, SupervisionError> {
	if let Some(digest) = &target.executable_sha256 {
		return Ok(digest.clone());
	}
	let built: BuiltRuntime =
		serde_json::from_str(built).map_err(|_| SupervisionError::PreflightFailed)?;
	if built.upstream_commit != lock.upstream_commit || built.schema_sha256 != target.schema_sha256
	{
		return Err(SupervisionError::PreflightFailed);
	}
	Ok(built.executable_sha256)
}

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
	validate_image(
		&command.program,
		&expected,
		&hex_digest(&command.executable_digest),
		&executable_digest(&lock, target, BUILT)?,
	)?;
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
	expected_digest: &str,
) -> Result<(), SupervisionError> {
	if program != expected.canonicalize().map_err(|_| SupervisionError::ExecutableUnavailable)?
		|| digest != expected_digest
	{
		return Err(SupervisionError::ExecutableChanged);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn source_runtime_requires_build_evidence_for_the_exact_commit_and_schema() {
		let target = Target { executable_sha256: None, schema_sha256: "schema".into() };
		let lock = RuntimeLock {
			upstream_commit: "reviewed".into(),
			version: "0.0.0".into(),
			targets: BTreeMap::new(),
		};
		let evidence = serde_json::json!({"upstreamCommit":"reviewed","executableSha256":"built-image","schemaSha256":"schema"});
		assert_eq!(
			executable_digest(&lock, &target, &evidence.to_string()).unwrap(),
			"built-image"
		);
		assert!(executable_digest(&lock, &target, "{}").is_err());
		for key in ["upstreamCommit", "schemaSha256"] {
			let mut stale = evidence.clone();
			stale[key] = "another-build".into();
			assert!(executable_digest(&lock, &target, &stale.to_string()).is_err());
		}
		let official = Target {
			executable_sha256: Some("official-image".into()),
			schema_sha256: "schema".into(),
		};
		assert_eq!(executable_digest(&lock, &official, "{}").unwrap(), "official-image");
	}

	#[test]
	fn bundled_identity_rejects_another_binary_or_digest() {
		let root = tempfile::tempdir().unwrap();
		let path = root.path().join("codex");
		std::fs::write(&path, b"fixture").unwrap();
		let canonical = path.canonicalize().unwrap();
		let target = "expected";
		assert!(validate_image(&canonical, &path, "expected", target).is_ok());
		assert!(validate_image(&canonical, &path, "changed", target).is_err());
		assert!(validate_image(Path::new("/another/codex"), &path, "expected", target).is_err());
	}
}
