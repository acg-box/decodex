//! Read the registry manifest without pulling layers or changing the managed service.
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use super::service::docker;
use crate::{ComponentStore, Result, hex};

pub(super) const IMAGE: &str = "infiniflow/ragflow:nightly";

/// Exact platform manifest and the installed version it was compared with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Update {
	/// Version reported by the running service at installation.
	pub version: String,
	/// Local image identity at discovery.
	pub installed_image: String,
	/// Immutable upstream platform manifest.
	pub digest: String,
}

impl ComponentStore {
	/// Discover a candidate only for a completed local installation.
	pub fn ragflow_update(&self) -> Result<Option<Update>> {
		let Some(installation) =
			self.ragflow_installation()?.filter(|i| i.installed && !i.image.is_empty())
		else {
			return Ok(None);
		};
		let docker = docker()?;
		let inspect = |args: &[&str]| {
			let output = Command::new(&docker).args(args).output().map_err(|e| e.to_string())?;
			if !output.status.success() {
				return Err(
					"Could not check the RAGFlow image. Check Docker and the registry connection."
						.to_owned(),
				);
			}
			Ok(output.stdout)
		};
		let local = inspect(&[
			"image",
			"inspect",
			"--format",
			"{{.Os}}/{{.Architecture}} {{.Id}}",
			&installation.image,
		])?;
		let local = std::str::from_utf8(&local).map_err(|_| "Invalid local image identity.")?;
		let (platform, id) = local.trim().split_once(' ').ok_or("Missing local image platform.")?;
		let remote = inspect(&["manifest", "inspect", "--verbose", IMAGE])?;
		Ok(candidate(platform, id, &remote)?.map(|digest| Update {
			version: installation.version,
			installed_image: installation.image,
			digest,
		}))
	}
}

pub(super) fn valid_digest(value: &str) -> bool {
	value.strip_prefix("sha256:").is_some_and(|id| hex(id, 64))
}

fn candidate(platform: &str, local: &str, remote: &[u8]) -> Result<Option<String>> {
	#[derive(Deserialize)]
	struct Descriptor {
		digest: String,
		platform: Platform,
	}
	#[derive(Deserialize)]
	struct Platform {
		os: String,
		architecture: String,
	}
	let manifests: Value =
		serde_json::from_slice(remote).map_err(|_| "Invalid RAGFlow registry manifest.")?;
	let (os, arch) = platform.split_once('/').ok_or("Invalid local image platform.")?;
	for entry in manifests.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(&manifests))
	{
		let Ok(descriptor) = serde_json::from_value::<Descriptor>(entry["Descriptor"].clone())
		else {
			continue;
		};
		if descriptor.platform.os != os || descriptor.platform.architecture != arch {
			continue;
		}
		let config = entry
			.pointer("/OCIManifest/config/digest")
			.or_else(|| entry.pointer("/SchemaV2Manifest/config/digest"))
			.and_then(Value::as_str)
			.ok_or("RAGFlow manifest has no image configuration.")?;
		if ![config, local, &descriptor.digest].into_iter().all(valid_digest) {
			return Err("Invalid RAGFlow image digest.".into());
		}
		return Ok((config != local && descriptor.digest != local).then_some(descriptor.digest));
	}
	Err("The RAGFlow registry did not report an image for the installed platform.".into())
}

#[cfg(test)]
mod tests {
	use super::candidate;
	#[test]
	fn select_the_installed_platform_and_compare_its_config_digest() {
		let digest = |c: char| format!("sha256:{}", c.to_string().repeat(64));
		let entries = [("amd64", digest('a'), digest('b')), ("arm64", digest('c'), digest('d'))]
			.map(|(arch, manifest, config)| {
				serde_json::json!({
					"Descriptor":{"digest":manifest,"platform":{"os":"linux","architecture":arch}},
					"OCIManifest":{"config":{"digest":config}}
				})
			});
		let remote = serde_json::to_vec(&entries).unwrap();
		assert_eq!(candidate("linux/arm64", &digest('d'), &remote), Ok(None));
		assert_eq!(candidate("linux/arm64", &digest('b'), &remote), Ok(Some(digest('c'))));
		assert!(candidate("linux/riscv64", &digest('a'), &remote).is_err());
	}
}
