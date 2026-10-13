//! Select an official release without dropping the reviewed development baseline.

use crate::{
	InstallProgress, Result, RuntimeLock, RuntimeTarget,
	install::{Archive, prepare_package},
};
use serde_json::Value;
use std::{collections::BTreeMap, process::Command};

/// Resolve and verify one explicit upstream release. Never select a floating version.
pub fn pin_release(
	version: &str,
	baseline: &RuntimeLock,
	mut progress: impl FnMut(InstallProgress),
) -> Result<RuntimeLock> {
	semver::Version::parse(version).map_err(|e| e.to_string())?;
	baseline.validate()?;
	if !cfg!(target_os = "macos") {
		return Err("Pin a signed Codex macOS release on macOS.".into());
	}
	let tag = format!("rust-v{version}");
	let release = github(&format!("releases/tags/{tag}"))?;
	if release["draft"] != false || release["tag_name"] != tag {
		return Err("Expected a published official Codex release.".into());
	}
	let mut reference = github(&format!("git/ref/tags/{tag}"))?["object"].clone();
	while reference["type"] == "tag" {
		let sha = reference["sha"].as_str().ok_or("Missing release tag identity.")?;
		reference = github(&format!("git/tags/{sha}"))?["object"].clone();
	}
	let commit = reference["sha"].as_str().ok_or("Missing release commit.")?;
	if reference["type"] != "commit" || !crate::hex(commit, 40) {
		return Err("The official release commit is unresolved.".into());
	}
	let comparison = github(&format!("compare/{}...{commit}", baseline.upstream_commit))?;
	require_coverage(&comparison)?;
	let platform = crate::platform();
	let name = format!("codex-provisioned-package-{platform}.tar.gz");
	let url = format!("https://github.com/openai/codex/releases/download/{tag}/{name}");
	let asset = release["assets"]
		.as_array()
		.and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
		.ok_or("The release has no signed package for this platform.")?;
	let sha256 = asset["digest"]
		.as_str()
		.and_then(|digest| digest.strip_prefix("sha256:"))
		.filter(|digest| crate::hex(digest, 64))
		.ok_or("The official release has no SHA-256 digest.")?;
	if asset["browser_download_url"] != url {
		return Err("The release asset URL is not canonical.".into());
	}
	let mut lock = RuntimeLock {
		version: version.into(),
		tag: Some(tag),
		upstream_commit: commit.into(),
		source: None,
		preserves_upstream_commit: Some(baseline.upstream_commit.clone()),
		targets: BTreeMap::from([(
			platform.into(),
			RuntimeTarget {
				url: url.clone(),
				sha256: sha256.into(),
				executable_sha256: String::new(),
				schema_sha256: String::new(),
			},
		)]),
	};
	let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
	let (executable_sha256, schema_sha256) =
		prepare_package(&lock, &Archive::Remote(url), sha256, stage.path(), &mut progress)?;
	let target = lock.targets.get_mut(platform).ok_or("Missing release platform.")?;
	target.executable_sha256 = executable_sha256;
	target.schema_sha256 = schema_sha256;
	lock.validate()?;
	Ok(lock)
}

fn github(path: &str) -> Result<Value> {
	let output = Command::new("gh")
		.args(["api", &format!("repos/openai/codex/{path}")])
		.output()
		.map_err(|e| e.to_string())?;
	if !output.status.success() {
		return Err(format!(
			"Cannot read the official Codex release: {}",
			String::from_utf8_lossy(&output.stderr)
		));
	}
	serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn require_coverage(comparison: &Value) -> Result<()> {
	if !matches!(comparison["status"].as_str(), Some("ahead" | "identical")) {
		return Err("This official release does not include the current development baseline. Keep the development pin; do not discard merged fixes.".into());
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn older_or_diverged_releases_cannot_replace_merged_fixes() {
		for status in ["behind", "diverged", "unknown"] {
			assert!(require_coverage(&serde_json::json!({"status": status})).is_err());
		}
		for status in ["ahead", "identical"] {
			assert!(require_coverage(&serde_json::json!({"status": status})).is_ok());
		}
	}
}
