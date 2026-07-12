//! Canonical predecessor patch evidence computed directly from Git objects.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::Path,
};

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
pub struct CommitEvidence {
	pub oid: Vec<u8>,
	pub parent_oids: Vec<Vec<u8>>,
	pub tree_oid: Vec<u8>,
	pub is_empty: bool,
	pub is_merge: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathTransition {
	pub commit_oid: Vec<u8>,
	pub old: Option<TreeEntryEvidence>,
	pub new: Option<TreeEntryEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetZeroPathHistory {
	pub path: Vec<u8>,
	pub transitions: Vec<PathTransition>,
	pub patch_unit_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EmptyCommitUnit {
	pub commit_oid: Vec<u8>,
	pub patch_unit_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergeTopologyUnit {
	pub commit_oid: Vec<u8>,
	pub parent_oids: Vec<Vec<u8>>,
	pub tree_oid: Vec<u8>,
	pub patch_unit_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CanonicalPatchSet {
	pub schema: String,
	pub object_format: GitObjectFormat,
	pub merge_base_oid: Vec<u8>,
	pub head_oid: Vec<u8>,
	pub commits: Vec<CommitEvidence>,
	pub path_deltas: Vec<PathDelta>,
	pub net_zero_path_histories: Vec<NetZeroPathHistory>,
	pub empty_commits: Vec<EmptyCommitUnit>,
	pub merge_topologies: Vec<MergeTopologyUnit>,
	pub digest: String,
}
impl CanonicalPatchSet {
	pub fn patch_unit_digests(&self) -> BTreeSet<String> {
		self.path_deltas
			.iter()
			.map(|unit| unit.patch_unit_digest.clone())
			.chain(self.net_zero_path_histories.iter().map(|unit| unit.patch_unit_digest.clone()))
			.chain(self.empty_commits.iter().map(|unit| unit.patch_unit_digest.clone()))
			.chain(self.merge_topologies.iter().map(|unit| unit.patch_unit_digest.clone()))
			.collect()
	}

	pub fn head_oid_hex(&self) -> String {
		hex_bytes(&self.head_oid)
	}

	pub fn merge_base_oid_hex(&self) -> String {
		hex_bytes(&self.merge_base_oid)
	}

	pub fn ordered_commit_oids_hex(&self) -> Vec<String> {
		self.commits.iter().map(|commit| hex_bytes(&commit.oid)).collect()
	}
}

#[derive(Debug)]
pub enum PatchSetBuildError {
	Open(gix::open::Error),
	InvalidOid { name: &'static str, value: String },
	Object(String),
	MultipleBestMergeBases,
	NoMergeBase,
	CommitGraphCycle,
	ShallowRepository,
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
			Self::NoMergeBase => formatter.write_str("commits have no merge base"),
			Self::CommitGraphCycle => {
				formatter.write_str("commit graph did not produce a complete topological order")
			},
			Self::ShallowRepository => {
				formatter.write_str("shallow Git repositories cannot produce canonical PatchSets")
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
	if repo.is_shallow() {
		return Err(PatchSetBuildError::ShallowRepository);
	}
	let base = parse_oid("base", base_oid)?;
	let head = parse_oid("head", head_oid)?;
	let bases = repo
		.merge_bases_many(head, &[base])
		.map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	if bases.is_empty() {
		return Err(PatchSetBuildError::NoMergeBase);
	}
	if bases.len() != 1 {
		return Err(PatchSetBuildError::MultipleBestMergeBases);
	}
	let merge_base = bases[0].detach();
	let ordered_commits = ordered_commit_set(&repo, merge_base, head)?;
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
	let endpoint_paths = path_deltas
		.iter()
		.map(|delta| {
			delta.old.as_ref().or(delta.new.as_ref()).expect("delta has one side").path.clone()
		})
		.collect::<BTreeSet<_>>();
	let (commits, transitions) = commit_evidence_and_transitions(&repo, &ordered_commits)?;
	let net_zero_path_histories = transitions
		.into_iter()
		.filter(|(path, records)| !endpoint_paths.contains(path) && !records.is_empty())
		.map(|(path, transitions)| {
			let patch_unit_digest = digest_net_zero(&path, &transitions);
			NetZeroPathHistory { path, transitions, patch_unit_digest }
		})
		.collect();
	let empty_commits = commits
		.iter()
		.filter(|commit| commit.is_empty)
		.map(|commit| EmptyCommitUnit {
			commit_oid: commit.oid.clone(),
			patch_unit_digest: digest_empty_commit(&commit.oid),
		})
		.collect();
	let merge_topologies = commits
		.iter()
		.filter(|commit| commit.is_merge)
		.map(|commit| MergeTopologyUnit {
			commit_oid: commit.oid.clone(),
			parent_oids: commit.parent_oids.clone(),
			tree_oid: commit.tree_oid.clone(),
			patch_unit_digest: digest_merge_topology(
				&commit.oid,
				&commit.parent_oids,
				&commit.tree_oid,
			),
		})
		.collect();
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
		commits,
		path_deltas,
		net_zero_path_histories,
		empty_commits,
		merge_topologies,
		digest: String::new(),
	};
	patch_set.digest = hex_digest(&canonical_patch_set_bytes(&patch_set));
	Ok(patch_set)
}

#[derive(Clone)]
struct RawCommit {
	oid: gix::ObjectId,
	parents: Vec<gix::ObjectId>,
	tree: gix::ObjectId,
}

fn load_commit(
	repo: &gix::Repository,
	oid: gix::ObjectId,
) -> Result<RawCommit, PatchSetBuildError> {
	let commit =
		repo.find_commit(oid).map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
	let parents = commit.parent_ids().map(|parent| parent.detach()).collect();
	let tree =
		commit.tree_id().map_err(|error| PatchSetBuildError::Object(error.to_string()))?.detach();
	Ok(RawCommit { oid, parents, tree })
}

fn reachable(
	repo: &gix::Repository,
	start: gix::ObjectId,
) -> Result<BTreeSet<gix::ObjectId>, PatchSetBuildError> {
	let mut seen = BTreeSet::new();
	let mut pending = vec![start];
	while let Some(oid) = pending.pop() {
		if !seen.insert(oid) {
			continue;
		}
		pending.extend(load_commit(repo, oid)?.parents);
	}
	Ok(seen)
}

fn ordered_commit_set(
	repo: &gix::Repository,
	merge_base: gix::ObjectId,
	head: gix::ObjectId,
) -> Result<Vec<RawCommit>, PatchSetBuildError> {
	let excluded = reachable(repo, merge_base)?;
	let mut candidates = BTreeMap::new();
	let mut pending = vec![head];
	while let Some(oid) = pending.pop() {
		if excluded.contains(&oid) || candidates.contains_key(&oid) {
			continue;
		}
		let commit = load_commit(repo, oid)?;
		pending.extend(commit.parents.iter().copied());
		candidates.insert(oid, commit);
	}
	let mut indegree = BTreeMap::new();
	let mut children = BTreeMap::<gix::ObjectId, BTreeSet<gix::ObjectId>>::new();
	for (oid, commit) in &candidates {
		let mut count = 0_usize;
		for parent in commit.parents.iter().filter(|parent| candidates.contains_key(*parent)) {
			count += 1;
			children.entry(*parent).or_default().insert(*oid);
		}
		indegree.insert(*oid, count);
	}
	let mut ready = indegree
		.iter()
		.filter_map(|(oid, count)| (*count == 0).then_some(*oid))
		.collect::<BTreeSet<_>>();
	let mut ordered = Vec::with_capacity(candidates.len());
	while let Some(oid) = ready.pop_first() {
		ordered.push(candidates.get(&oid).expect("ready commit exists").clone());
		for child in children.get(&oid).into_iter().flatten() {
			let count = indegree.get_mut(child).expect("child indegree exists");
			*count -= 1;
			if *count == 0 {
				ready.insert(*child);
			}
		}
	}
	if ordered.len() != candidates.len() {
		return Err(PatchSetBuildError::CommitGraphCycle);
	}
	Ok(ordered)
}

fn commit_evidence_and_transitions(
	repo: &gix::Repository,
	ordered: &[RawCommit],
) -> Result<(Vec<CommitEvidence>, BTreeMap<Vec<u8>, Vec<PathTransition>>), PatchSetBuildError> {
	let empty_tree = gix::ObjectId::empty_tree(repo.object_hash());
	let mut tree_cache = BTreeMap::new();
	let mut evidence = Vec::with_capacity(ordered.len());
	let mut histories = BTreeMap::<Vec<u8>, Vec<PathTransition>>::new();
	for commit in ordered {
		let first_parent_tree = match commit.parents.first() {
			Some(parent) => load_commit(repo, *parent)?.tree,
			None => empty_tree,
		};
		let old_entries = flattened_tree_by_oid(repo, first_parent_tree, &mut tree_cache)?.clone();
		let new_entries = flattened_tree_by_oid(repo, commit.tree, &mut tree_cache)?;
		let mut paths = old_entries.keys().chain(new_entries.keys()).cloned().collect::<Vec<_>>();
		paths.sort();
		paths.dedup();
		for path in paths {
			let old = old_entries.get(&path).cloned();
			let new = new_entries.get(&path).cloned();
			if old != new {
				histories.entry(path).or_default().push(PathTransition {
					commit_oid: commit.oid.as_bytes().to_vec(),
					old,
					new,
				});
			}
		}
		evidence.push(CommitEvidence {
			oid: commit.oid.as_bytes().to_vec(),
			parent_oids: commit.parents.iter().map(|oid| oid.as_bytes().to_vec()).collect(),
			tree_oid: commit.tree.as_bytes().to_vec(),
			is_empty: commit.tree == first_parent_tree,
			is_merge: commit.parents.len() > 1,
		});
	}
	Ok((evidence, histories))
}

fn flattened_tree_by_oid<'a>(
	repo: &gix::Repository,
	oid: gix::ObjectId,
	cache: &'a mut BTreeMap<gix::ObjectId, BTreeMap<Vec<u8>, TreeEntryEvidence>>,
) -> Result<&'a BTreeMap<Vec<u8>, TreeEntryEvidence>, PatchSetBuildError> {
	if !cache.contains_key(&oid) {
		let object =
			repo.find_object(oid).map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
		let tree = object
			.try_into_tree()
			.map_err(|error| PatchSetBuildError::Object(error.to_string()))?;
		cache.insert(oid, flatten_tree(&tree)?);
	}
	Ok(cache.get(&oid).expect("tree cache populated"))
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
				kind: canonical_object_kind(mode.kind()).to_owned(),
				mode: u32::from(mode.value()),
				oid: entry.oid.as_bytes().to_vec(),
			};
			(path, evidence)
		})
		.collect())
}

fn canonical_object_kind(kind: gix::object::tree::EntryKind) -> &'static str {
	match kind {
		gix::object::tree::EntryKind::Tree => "tree",
		gix::object::tree::EntryKind::Commit => "commit",
		gix::object::tree::EntryKind::Blob
		| gix::object::tree::EntryKind::BlobExecutable
		| gix::object::tree::EntryKind::Link => "blob",
	}
}

fn digest_path_delta(old: Option<&TreeEntryEvidence>, new: Option<&TreeEntryEvidence>) -> String {
	let mut bytes = Vec::new();
	put_field(&mut bytes, b"decodex.patch_unit/path_delta/1");
	put_entry(&mut bytes, old);
	put_entry(&mut bytes, new);
	hex_digest(&bytes)
}

fn digest_net_zero(path: &[u8], transitions: &[PathTransition]) -> String {
	let mut bytes = Vec::new();
	put_field(&mut bytes, b"decodex.patch_unit/net_zero_path_history/1");
	put_field(&mut bytes, path);
	put_u64(&mut bytes, transitions.len() as u64);
	for transition in transitions {
		put_field(&mut bytes, &transition.commit_oid);
		put_entry(&mut bytes, transition.old.as_ref());
		put_entry(&mut bytes, transition.new.as_ref());
	}
	hex_digest(&bytes)
}

fn digest_empty_commit(commit_oid: &[u8]) -> String {
	let mut bytes = Vec::new();
	put_field(&mut bytes, b"decodex.patch_unit/empty_commit/1");
	put_field(&mut bytes, commit_oid);
	hex_digest(&bytes)
}

fn digest_merge_topology(commit_oid: &[u8], parents: &[Vec<u8>], tree_oid: &[u8]) -> String {
	let mut bytes = Vec::new();
	put_field(&mut bytes, b"decodex.patch_unit/merge_topology/1");
	put_field(&mut bytes, commit_oid);
	put_u64(&mut bytes, parents.len() as u64);
	for parent in parents {
		put_field(&mut bytes, parent);
	}
	put_field(&mut bytes, tree_oid);
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
	put_u64(&mut bytes, patch_set.commits.len() as u64);
	for commit in &patch_set.commits {
		put_field(&mut bytes, &commit.oid);
		put_u64(&mut bytes, commit.parent_oids.len() as u64);
		for parent in &commit.parent_oids {
			put_field(&mut bytes, parent);
		}
		put_field(&mut bytes, &commit.tree_oid);
		bytes.push(u8::from(commit.is_empty));
		bytes.push(u8::from(commit.is_merge));
	}
	put_u64(&mut bytes, patch_set.path_deltas.len() as u64);
	for delta in &patch_set.path_deltas {
		put_entry(&mut bytes, delta.old.as_ref());
		put_entry(&mut bytes, delta.new.as_ref());
		put_field(&mut bytes, delta.patch_unit_digest.as_bytes());
	}
	put_u64(&mut bytes, patch_set.net_zero_path_histories.len() as u64);
	for history in &patch_set.net_zero_path_histories {
		put_field(&mut bytes, &history.path);
		put_u64(&mut bytes, history.transitions.len() as u64);
		for transition in &history.transitions {
			put_field(&mut bytes, &transition.commit_oid);
			put_entry(&mut bytes, transition.old.as_ref());
			put_entry(&mut bytes, transition.new.as_ref());
		}
		put_field(&mut bytes, history.patch_unit_digest.as_bytes());
	}
	put_u64(&mut bytes, patch_set.empty_commits.len() as u64);
	for unit in &patch_set.empty_commits {
		put_field(&mut bytes, &unit.commit_oid);
		put_field(&mut bytes, unit.patch_unit_digest.as_bytes());
	}
	put_u64(&mut bytes, patch_set.merge_topologies.len() as u64);
	for unit in &patch_set.merge_topologies {
		put_field(&mut bytes, &unit.commit_oid);
		put_u64(&mut bytes, unit.parent_oids.len() as u64);
		for parent in &unit.parent_oids {
			put_field(&mut bytes, parent);
		}
		put_field(&mut bytes, &unit.tree_oid);
		put_field(&mut bytes, unit.patch_unit_digest.as_bytes());
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
	hex_bytes(&Sha256::digest(bytes))
}

fn hex_bytes(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
		let submodule_entry = format!("160000,{base},submodule");
		git(fixture.path(), &["update-index", "--add", "--cacheinfo", &submodule_entry]);
		git(fixture.path(), &["commit", "-qm", "head"]);
		let head = git_output(fixture.path(), &["rev-parse", "HEAD"]);

		let first =
			build_canonical_patch_set(fixture.path(), &base, &head).expect("canonical patch set");
		let second =
			build_canonical_patch_set(fixture.path(), &base, &head).expect("stable patch set");
		assert_eq!(first, second);
		assert_eq!(first.path_deltas.len(), 5);
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
		let submodule = first
			.path_deltas
			.iter()
			.find_map(|delta| delta.new.as_ref().filter(|entry| entry.path == b"submodule"))
			.expect("submodule delta");
		assert_eq!(submodule.kind, "commit");
		assert_eq!(submodule.mode, 0o160000);
		let handoff = super::super::RepairHandoffAuthority::new(
			"handoff",
			"github:helixbox/pubfi-mono",
			super::super::LaneId::new("pubfi", "predecessor").expect("lane"),
			"PUB-1704",
			"https://github.com/helixbox/pubfi-mono/pull/826",
			&head,
			7,
			"refs/heads/main",
			&base,
			&first,
			super::super::LaneId::new("pubfi", "successor").expect("lane"),
			"PUB-1705",
			"findings",
			"checkpoint",
			"operator",
			"event",
		)
		.expect("canonical handoff");
		handoff.validate().expect("valid handoff");
		assert!(
			super::super::RepairHandoffAuthority::new(
				"handoff",
				"github:helixbox/pubfi-mono",
				super::super::LaneId::new("pubfi", "predecessor").expect("lane"),
				"PUB-1704",
				"https://github.com/helixbox/pubfi-mono/pull/826",
				"changed-head",
				7,
				"refs/heads/main",
				&base,
				&first,
				super::super::LaneId::new("pubfi", "successor").expect("lane"),
				"PUB-1705",
				"findings",
				"checkpoint",
				"operator",
				"event",
			)
			.is_err()
		);
	}

	#[test]
	fn canonical_commit_dag_emits_net_zero_empty_and_merge_units() {
		let fixture = tempfile::tempdir().expect("temporary repository");
		git(fixture.path(), &["init", "-q"]);
		git(fixture.path(), &["config", "user.name", "Fixture"]);
		git(fixture.path(), &["config", "user.email", "fixture@example.com"]);
		git(fixture.path(), &["config", "core.hooksPath", ".git/no-hooks"]);
		fs::write(fixture.path().join("cycle"), b"base\n").expect("base file");
		git(fixture.path(), &["add", "cycle"]);
		git(fixture.path(), &["commit", "-qm", "base"]);
		let base = git_output(fixture.path(), &["rev-parse", "HEAD"]);
		let default_branch = git_output(fixture.path(), &["branch", "--show-current"]);

		fs::write(fixture.path().join("cycle"), b"middle\n").expect("middle file");
		git(fixture.path(), &["commit", "-qam", "change cycle"]);
		fs::write(fixture.path().join("cycle"), b"base\n").expect("restored file");
		git(fixture.path(), &["commit", "-qam", "restore cycle"]);
		git(fixture.path(), &["commit", "--allow-empty", "-qm", "empty"]);
		git(fixture.path(), &["checkout", "-qb", "side"]);
		fs::write(fixture.path().join("side-file"), b"side\n").expect("side file");
		git(fixture.path(), &["add", "side-file"]);
		git(fixture.path(), &["commit", "-qm", "side"]);
		git(fixture.path(), &["checkout", "-q", &default_branch]);
		fs::write(fixture.path().join("main-file"), b"main\n").expect("main file");
		git(fixture.path(), &["add", "main-file"]);
		git(fixture.path(), &["commit", "-qm", "main"]);
		git(fixture.path(), &["merge", "--no-ff", "-qm", "merge side", "side"]);
		let head = git_output(fixture.path(), &["rev-parse", "HEAD"]);

		let patch_set = build_canonical_patch_set(fixture.path(), &base, &head).expect("patch set");
		assert_eq!(patch_set.net_zero_path_histories.len(), 1);
		assert_eq!(patch_set.net_zero_path_histories[0].path, b"cycle");
		assert_eq!(patch_set.net_zero_path_histories[0].transitions.len(), 2);
		assert_eq!(patch_set.empty_commits.len(), 1);
		assert_eq!(patch_set.merge_topologies.len(), 1);
		let merge = &patch_set.merge_topologies[0];
		assert_eq!(merge.parent_oids.len(), 2);
		let positions = patch_set
			.commits
			.iter()
			.enumerate()
			.map(|(index, commit)| (commit.oid.clone(), index))
			.collect::<BTreeMap<_, _>>();
		for commit in &patch_set.commits {
			for parent in &commit.parent_oids {
				if let Some(parent_index) = positions.get(parent) {
					assert!(parent_index < positions.get(&commit.oid).expect("commit index"));
				}
			}
		}
		assert_eq!(
			merge.parent_oids,
			git_output(fixture.path(), &["show", "-s", "--format=%P", &head])
				.split_whitespace()
				.map(|oid| {
					gix::ObjectId::from_hex(oid.as_bytes()).expect("parent oid").as_bytes().to_vec()
				})
				.collect::<Vec<_>>()
		);
	}

	#[test]
	fn multiple_best_merge_bases_are_rejected() {
		let fixture = initialized_repository();
		let base = git_output(fixture.path(), &["rev-parse", "HEAD"]);
		git(fixture.path(), &["checkout", "-qb", "left"]);
		fs::write(fixture.path().join("left"), b"left\n").expect("left file");
		git(fixture.path(), &["add", "left"]);
		git(fixture.path(), &["commit", "-qm", "left"]);
		let left = git_output(fixture.path(), &["rev-parse", "HEAD"]);
		git(fixture.path(), &["checkout", "-q", "--detach", &base]);
		fs::write(fixture.path().join("right"), b"right\n").expect("right file");
		git(fixture.path(), &["add", "right"]);
		git(fixture.path(), &["commit", "-qm", "right"]);
		let right = git_output(fixture.path(), &["rev-parse", "HEAD"]);
		let tree = git_output(fixture.path(), &["rev-parse", &format!("{left}^{{tree}}")]);
		let merge_one = git_output(
			fixture.path(),
			&["commit-tree", &tree, "-p", &left, "-p", &right, "-m", "merge one"],
		);
		let merge_two = git_output(
			fixture.path(),
			&["commit-tree", &tree, "-p", &right, "-p", &left, "-m", "merge two"],
		);
		assert!(matches!(
			build_canonical_patch_set(fixture.path(), &merge_two, &merge_one),
			Err(PatchSetBuildError::MultipleBestMergeBases)
		));
	}

	#[test]
	fn octopus_parent_order_and_non_first_parent_empty_rule_are_canonical() {
		let fixture = initialized_repository();
		let base = git_output(fixture.path(), &["rev-parse", "HEAD"]);
		let mut parents = Vec::new();
		for branch in ["one", "two", "three"] {
			git(fixture.path(), &["checkout", "-q", "--detach", &base]);
			fs::write(fixture.path().join(branch), format!("{branch}\n")).expect("branch file");
			git(fixture.path(), &["add", branch]);
			git(fixture.path(), &["commit", "-qm", branch]);
			parents.push(git_output(fixture.path(), &["rev-parse", "HEAD"]));
		}
		let second_parent_tree =
			git_output(fixture.path(), &["rev-parse", &format!("{}^{{tree}}", parents[1])]);
		let head = git_output(
			fixture.path(),
			&[
				"commit-tree",
				&second_parent_tree,
				"-p",
				&parents[0],
				"-p",
				&parents[1],
				"-p",
				&parents[2],
				"-m",
				"octopus",
			],
		);
		let patch_set = build_canonical_patch_set(fixture.path(), &base, &head).expect("patch set");
		let head_bytes = oid_bytes(&head);
		let merge = patch_set
			.merge_topologies
			.iter()
			.find(|unit| unit.commit_oid == head_bytes)
			.expect("octopus topology");
		assert_eq!(merge.parent_oids, parents.iter().map(|oid| oid_bytes(oid)).collect::<Vec<_>>());
		assert!(!patch_set.empty_commits.iter().any(|unit| unit.commit_oid == head_bytes));
		let sibling_order =
			patch_set.commits[..3].iter().map(|commit| commit.oid.clone()).collect::<Vec<_>>();
		let mut sorted_parent_oids = parents.iter().map(|oid| oid_bytes(oid)).collect::<Vec<_>>();
		sorted_parent_oids.sort();
		assert_eq!(sibling_order, sorted_parent_oids);
	}

	#[test]
	fn shallow_and_missing_object_histories_are_rejected() {
		let source = initialized_repository();
		let base = git_output(source.path(), &["rev-parse", "HEAD"]);
		fs::write(source.path().join("second"), b"second\n").expect("second file");
		git(source.path(), &["add", "second"]);
		git(source.path(), &["commit", "-qm", "second"]);
		let head = git_output(source.path(), &["rev-parse", "HEAD"]);
		let head_tree = git_output(source.path(), &["rev-parse", "HEAD^{tree}"]);
		let tree_object =
			source.path().join(".git/objects").join(&head_tree[..2]).join(&head_tree[2..]);
		fs::remove_file(tree_object).expect("remove loose tree object");
		assert!(matches!(
			build_canonical_patch_set(source.path(), &base, &head),
			Err(PatchSetBuildError::Object(_))
		));

		let complete = initialized_repository();
		let shallow_parent = tempfile::tempdir().expect("shallow parent");
		let shallow = shallow_parent.path().join("shallow");
		let source_url = format!("file://{}", complete.path().display());
		let shallow_path = shallow.to_string_lossy().into_owned();
		git(shallow_parent.path(), &["clone", "-q", "--depth", "1", &source_url, &shallow_path]);
		let shallow_head = git_output(&shallow, &["rev-parse", "HEAD"]);
		assert!(matches!(
			build_canonical_patch_set(&shallow, &shallow_head, &shallow_head),
			Err(PatchSetBuildError::ShallowRepository)
		));
	}

	fn initialized_repository() -> tempfile::TempDir {
		let fixture = tempfile::tempdir().expect("temporary repository");
		git(fixture.path(), &["init", "-q"]);
		git(fixture.path(), &["config", "user.name", "Fixture"]);
		git(fixture.path(), &["config", "user.email", "fixture@example.com"]);
		git(fixture.path(), &["config", "core.hooksPath", ".git/no-hooks"]);
		fs::write(fixture.path().join("base"), b"base\n").expect("base file");
		git(fixture.path(), &["add", "base"]);
		git(fixture.path(), &["commit", "-qm", "base"]);
		fixture
	}

	fn oid_bytes(oid: &str) -> Vec<u8> {
		gix::ObjectId::from_hex(oid.as_bytes()).expect("object id").as_bytes().to_vec()
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
