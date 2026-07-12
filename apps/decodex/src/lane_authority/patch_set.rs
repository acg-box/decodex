//! Canonical predecessor patch evidence computed directly from Git objects.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PATCH_SET_SCHEMA: &str = "decodex.patch_set/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitObjectFormat {
	Sha1,
	Sha256,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TreeEntryEvidence {
	pub path: Vec<u8>,
	pub kind: String,
	pub mode: u32,
	pub oid: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathDelta {
	pub old: Option<TreeEntryEvidence>,
	pub new: Option<TreeEntryEvidence>,
	pub patch_unit_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CanonicalPatchSet {
	pub schema: String,
	pub object_format: GitObjectFormat,
	pub merge_base_oid: Vec<u8>,
	pub head_oid: Vec<u8>,
	pub path_deltas: Vec<PathDelta>,
	pub digest: String,
}

#[derive(Debug)]
pub enum PatchSetBuildError {
	Open(gix::open::Error),
	InvalidOid { name: &'static str, value: String },
	Object(String),
	MultipleBestMergeBases,
	UnsupportedObjectFormat(String),
}
impl std::fmt::Display for PatchSetBuildError {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Open(error) => write!(formatter, "failed to open Git repository: {error}"),
			Self::InvalidOid { name, value } => {
				write!(formatter, "invalid {name} object id `{value}`")
			},
			Self::Object(error) => write!(formatter, "Git object is missing or corrupt: {error}"),
			Self::MultipleBestMergeBases => {
				formatter.write_str("multiple best merge bases are not canonical")
			},
			Self::UnsupportedObjectFormat(format) => {
				write!(formatter, "unsupported Git object format `{format}`")
			},
		}
	}
}
impl std::error::Error for PatchSetBuildError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::Open(error) => Some(error),
			_ => None,
		}
	}
}

pub fn build_canonical_patch_set(
	repository_path: &Path,
	base_oid: &str,
	head_oid: &str,
) -> Result<CanonicalPatchSet, PatchSetBuildError> {
	let repo = gix::open(repository_path).map_err(PatchSetBuildError::Open)?;
	let base = parse_oid("base", base_oid)?;
	let head = parse_oid("head", head_oid)?;
	let bases = repo
		.merge_bases_many(head, &[base])
		.map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	if bases.len() != 1 {
		return Err(PatchSetBuildError::MultipleBestMergeBases);
	}
	let merge_base = bases[0].detach();
	let base_commit = repo
		.find_commit(merge_base)
		.map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	let base_tree =
		base_commit.tree().map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	let head_commit =
		repo.find_commit(head).map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	let head_tree =
		head_commit.tree().map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	let old_entries = flatten_tree(&base_tree)?;
	let new_entries = flatten_tree(&head_tree)?;
	let mut paths = old_entries.keys().chain(new_entries.keys()).cloned().collect::<Vec<_>>();
	paths.sort();
	paths.dedup();
	let mut path_deltas = Vec::new();
	for path in paths {
		let old = old_entries.get(&path).cloned();
		let new = new_entries.get(&path).cloned();
		if old == new {
			continue;
		}
		let digest = digest_path_delta(old.as_ref(), new.as_ref());
		path_deltas.push(PathDelta { old, new, patch_unit_digest: digest });
	}
	let object_format = match repo.object_hash() {
		gix::hash::Kind::Sha1 => GitObjectFormat::Sha1,
		gix::hash::Kind::Sha256 => GitObjectFormat::Sha256,
		other => return Err(PatchSetBuildError::UnsupportedObjectFormat(format!("{other:?}"))),
	};
	let mut patch_set = CanonicalPatchSet {
		schema: PATCH_SET_SCHEMA.to_owned(),
		object_format,
		merge_base_oid: merge_base.as_bytes().to_vec(),
		head_oid: head.as_bytes().to_vec(),
		path_deltas,
		digest: String::new(),
	};
	patch_set.digest = hex_digest(&canonical_patch_set_bytes(&patch_set));
	Ok(patch_set)
}

fn parse_oid(name: &'static str, value: &str) -> Result<gix::ObjectId, PatchSetBuildError> {
	gix::ObjectId::from_hex(value.as_bytes())
		.map_err(|_| PatchSetBuildError::InvalidOid { name, value: value.to_owned() })
}

fn flatten_tree(
	tree: &gix::Tree<'_>,
) -> Result<BTreeMap<Vec<u8>, TreeEntryEvidence>, PatchSetBuildError> {
	let entries = tree
		.traverse()
		.breadthfirst
		.files()
		.map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	Ok(entries
		.into_iter()
		.map(|entry| {
			let path = entry.filepath.to_vec();
			let mode = entry.mode;
			let evidence = TreeEntryEvidence {
				path: path.clone(),
				kind: format!("{:?}", mode.kind()).to_ascii_lowercase(),
				mode: u32::from(mode.value()),
				oid: entry.oid.as_bytes().to_vec(),
			};
			(path, evidence)
		})
		.collect())
}

fn digest_path_delta(old: Option<&TreeEntryEvidence>, new: Option<&TreeEntryEvidence>) -> String {
	let mut bytes = Vec::new();
	put_field(&mut bytes, b"decodex.patch_unit/path_delta/1");
	put_entry(&mut bytes, old);
	put_entry(&mut bytes, new);
	hex_digest(&bytes)
}

fn canonical_patch_set_bytes(patch_set: &CanonicalPatchSet) -> Vec<u8> {
	let mut bytes = Vec::new();
	put_field(&mut bytes, PATCH_SET_SCHEMA.as_bytes());
	put_field(
		&mut bytes,
		match patch_set.object_format {
			GitObjectFormat::Sha1 => b"sha1",
			GitObjectFormat::Sha256 => b"sha256",
		},
	);
	put_field(&mut bytes, &patch_set.merge_base_oid);
	put_field(&mut bytes, &patch_set.head_oid);
	put_u64(&mut bytes, patch_set.path_deltas.len() as u64);
	for delta in &patch_set.path_deltas {
		put_entry(&mut bytes, delta.old.as_ref());
		put_entry(&mut bytes, delta.new.as_ref());
		put_field(&mut bytes, delta.patch_unit_digest.as_bytes());
	}
	bytes
}

fn put_entry(bytes: &mut Vec<u8>, entry: Option<&TreeEntryEvidence>) {
	match entry {
		None => bytes.push(0),
		Some(entry) => {
			bytes.push(1);
			put_field(bytes, &entry.path);
			put_field(bytes, entry.kind.as_bytes());
			bytes.extend_from_slice(&entry.mode.to_be_bytes());
			put_field(bytes, &entry.oid);
		},
	}
}

fn put_field(bytes: &mut Vec<u8>, value: &[u8]) {
	put_u64(bytes, value.len() as u64);
	bytes.extend_from_slice(value);
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
	bytes.extend_from_slice(&value.to_be_bytes());
}

fn hex_digest(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt, path::Path, process::Command};

	use super::*;

	#[test]
	fn canonical_endpoint_deltas_preserve_raw_paths_modes_and_binary_oids() {
		let fixture = tempfile::tempdir().expect("temporary repository");
		git(fixture.path(), &["init", "-q"]);
		git(fixture.path(), &["config", "user.name", "Fixture"]);
		git(fixture.path(), &["config", "user.email", "fixture@example.com"]);
		git(fixture.path(), &["config", "core.hooksPath", ".git/no-hooks"]);
		fs::write(fixture.path().join("rename-old"), [0, 159, 146, 150]).expect("binary fixture");
		fs::write(fixture.path().join("mode-only"), b"stable\n").expect("mode fixture");
		let unusual = OsString::from_vec(b"odd-\n-name".to_vec());
		fs::write(fixture.path().join(&unusual), b"before\n").expect("unusual path fixture");
		git(fixture.path(), &["add", "-A"]);
		git(fixture.path(), &["commit", "-qm", "base"]);
		let base = git_output(fixture.path(), &["rev-parse", "HEAD"]);

		fs::rename(fixture.path().join("rename-old"), fixture.path().join("rename-new"))
			.expect("rename fixture");
		let mut permissions =
			fs::metadata(fixture.path().join("mode-only")).expect("metadata").permissions();
		#[allow(clippy::permissions_set_readonly_false)]
		{
			use std::os::unix::fs::PermissionsExt;
			permissions.set_mode(0o755);
		}
		fs::set_permissions(fixture.path().join("mode-only"), permissions).expect("mode change");
		fs::write(fixture.path().join(&unusual), b"after\0binary\n").expect("unusual path update");
		git(fixture.path(), &["add", "-A"]);
		git(fixture.path(), &["commit", "-qm", "head"]);
		let head = git_output(fixture.path(), &["rev-parse", "HEAD"]);

		let first =
			build_canonical_patch_set(fixture.path(), &base, &head).expect("canonical patch set");
		let second =
			build_canonical_patch_set(fixture.path(), &base, &head).expect("stable patch set");
		assert_eq!(first, second);
		assert_eq!(first.path_deltas.len(), 4);
		assert!(
			first.path_deltas.iter().any(|delta| delta
				.old
				.as_ref()
				.is_some_and(|entry| entry.path == b"rename-old")
				&& delta.new.is_none())
		);
		assert!(
			first.path_deltas.iter().any(|delta| delta
				.new
				.as_ref()
				.is_some_and(|entry| entry.path == b"rename-new")
				&& delta.old.is_none())
		);
		let mode = first
			.path_deltas
			.iter()
			.find(|delta| delta.new.as_ref().is_some_and(|entry| entry.path == b"mode-only"))
			.expect("mode delta");
		assert_eq!(mode.old.as_ref().expect("old mode").mode, 0o100644);
		assert_eq!(mode.new.as_ref().expect("new mode").mode, 0o100755);
		assert!(
			first
				.path_deltas
				.iter()
				.any(|delta| delta.new.as_ref().is_some_and(|entry| entry.path == b"odd-\n-name"))
		);
	}

	fn git(repository: &Path, args: &[&str]) {
		let status = Command::new("git")
			.args(args)
			.current_dir(repository)
			.env("LC_ALL", "C")
			.status()
			.expect("git command");
		assert!(status.success(), "git {args:?} failed");
	}

	fn git_output(repository: &Path, args: &[&str]) -> String {
		let output = Command::new("git")
			.args(args)
			.current_dir(repository)
			.env("LC_ALL", "C")
			.output()
			.expect("git command");
		assert!(output.status.success(), "git {args:?} failed");
		String::from_utf8(output.stdout).expect("ASCII oid").trim().to_owned()
	}
}
