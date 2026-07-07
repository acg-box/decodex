use crate::docs_okf::{
	ErrorKind, OkfCheckProfile, OkfInitReport, OkfScaffoldFile, Path, PathBuf, Result, fs,
};

pub(crate) fn init_okf_bundle(root: &Path, profile: OkfCheckProfile) -> Result<OkfInitReport> {
	if profile == OkfCheckProfile::Decodex {
		color_eyre::eyre::bail!(
			"`decodex okf init` scaffolds portable profiles only; use Decodex docs policy for the `decodex` profile."
		);
	}

	let files = okf_scaffold_files(profile);

	ensure_scaffold_targets_available(root, &files)?;

	fs::create_dir_all(root)?;

	let mut report = OkfInitReport {
		profile,
		bundle_root: root.to_path_buf(),
		created: Vec::new(),
		unchanged: Vec::new(),
	};

	for file in files {
		write_scaffold_file(root, file.relative_path, file.content, &mut report)?;
	}

	Ok(report)
}

fn okf_scaffold_files(profile: OkfCheckProfile) -> Vec<OkfScaffoldFile> {
	vec![
		OkfScaffoldFile {
			relative_path: "index.md",
			content: "# OKF Bundle\n\n- [Overview](overview.md)\n\nUse this index to guide agents and humans to the smallest relevant concept.\n",
		},
		OkfScaffoldFile {
			relative_path: "log.md",
			content: "# OKF Log\n\n- Initialized this portable OKF bundle scaffold.\n",
		},
		OkfScaffoldFile { relative_path: "overview.md", content: overview_concept(profile) },
	]
}

fn ensure_scaffold_targets_available(root: &Path, files: &[OkfScaffoldFile]) -> Result<()> {
	for file in files {
		ensure_scaffold_target_available(root, file.relative_path, file.content)?;
	}

	Ok(())
}

fn ensure_scaffold_target_available(root: &Path, relative_path: &str, content: &str) -> Result<()> {
	let path = root.join(relative_path);

	match fs::read_to_string(&path) {
		Ok(existing) if existing == content => Ok(()),
		Ok(_) => reject_divergent_scaffold(&path),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn overview_concept(profile: OkfCheckProfile) -> &'static str {
	match profile {
		OkfCheckProfile::Core => {
			"---\ntype: Knowledge Bundle\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n"
		},
		OkfCheckProfile::Wiki => {
			"---\ntype: Knowledge Bundle\ntitle: OKF Bundle Overview\ndescription: Entry concept for the repository knowledge bundle.\ntags: [okf]\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n"
		},
		OkfCheckProfile::RepoMemory => {
			"---\ntype: Knowledge Bundle\ntitle: OKF Bundle Overview\ndescription: Entry concept for the repository knowledge bundle.\ntags: [okf, repo-memory]\nsource_refs: []\ncode_refs: []\nrelated: []\ndrift_watch: [decodex okf check, decodex okf graph]\n---\n\n# OKF Bundle Overview\n\nThis concept introduces the bundle and should be replaced with repository-specific knowledge.\n"
		},
		OkfCheckProfile::Decodex => unreachable!("decodex profile is rejected before scaffold"),
	}
}

fn write_scaffold_file(
	root: &Path,
	relative_path: &str,
	content: &str,
	report: &mut OkfInitReport,
) -> Result<()> {
	let path = root.join(relative_path);

	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	match fs::read_to_string(&path) {
		Ok(existing) if existing == content => {
			report.unchanged.push(PathBuf::from(relative_path));
		},
		Ok(_) => return reject_divergent_scaffold(&path),
		Err(error) if error.kind() == ErrorKind::NotFound => {
			fs::write(&path, content)?;

			report.created.push(PathBuf::from(relative_path));
		},
		Err(error) => return Err(error.into()),
	}

	Ok(())
}

fn reject_divergent_scaffold(path: &Path) -> Result<()> {
	color_eyre::eyre::bail!(
		"OKF scaffold target `{}` already exists with different content; move it or edit the bundle manually.",
		path.display()
	);
}
