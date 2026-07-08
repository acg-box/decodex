use std::{
	env, fs,
	path::{Path, PathBuf},
};

use clap::{Args, Subcommand};

use crate::prelude::{Result, eyre};

#[derive(Debug, Args)]
pub(in crate::cli) struct DocsCommand {
	#[command(subcommand)]
	command: DocsSubcommand,
}
impl DocsCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		match self.command {
			DocsSubcommand::Check => {
				let report = check_current_repo_docs()?;

				println!(
					"docs check ok: root={} markdown_files={}",
					report.repo_root.display(),
					report.markdown_file_count,
				);

				Ok(())
			},
		}
	}
}

#[derive(Debug, Subcommand)]
enum DocsSubcommand {
	/// Check the current repository docs as a portable Markdown bundle.
	Check,
}

#[derive(Debug)]
struct DocsCheckReport {
	repo_root: PathBuf,
	markdown_file_count: usize,
}

fn check_current_repo_docs() -> Result<DocsCheckReport> {
	check_docs_root(&env::current_dir()?)
}

fn check_docs_root(start: &Path) -> Result<DocsCheckReport> {
	let repo_root = find_docs_repo_root(start)?;
	let docs_root = repo_root.join("docs");
	let mut markdown_file_count = 0_usize;

	for file in markdown_files(&docs_root)? {
		let body = fs::read_to_string(&file).map_err(|error| {
			eyre::eyre!("failed to read docs file `{}`: {error}", file.display())
		})?;

		if body.contains('\0') {
			eyre::bail!("docs file `{}` contains a NUL byte", file.display());
		}

		markdown_file_count += 1;
	}

	if markdown_file_count == 0 {
		eyre::bail!("docs directory `{}` contains no Markdown files", docs_root.display());
	}

	Ok(DocsCheckReport { repo_root, markdown_file_count })
}

fn find_docs_repo_root(start: &Path) -> Result<PathBuf> {
	for candidate in start.ancestors() {
		if candidate.join("docs").is_dir() && candidate.join("docs/index.md").is_file() {
			return Ok(candidate.to_path_buf());
		}
	}

	eyre::bail!(
		"failed to find a repository docs root from `{}`; expected `docs/index.md` in this directory or one of its ancestors",
		start.display(),
	);
}

fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	collect_markdown_files(root, &mut files)?;

	files.sort();

	Ok(files)
}

fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
	for entry in fs::read_dir(root).map_err(|error| {
		eyre::eyre!("failed to read docs directory `{}`: {error}", root.display())
	})? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;

		if file_type.is_dir() {
			collect_markdown_files(&path, files)?;
		} else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "md")
		{
			files.push(path);
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{fs, time::SystemTime};

	fn temp_docs_root() -> std::path::PathBuf {
		let root = std::env::temp_dir().join(format!(
			"decodex-docs-check-test-{}",
			SystemTime::now()
				.duration_since(SystemTime::UNIX_EPOCH)
				.expect("time should be monotonic")
				.as_nanos()
		));

		fs::create_dir_all(root.join("docs")).expect("docs directory should create");

		root
	}

	#[test]
	fn docs_check_accepts_markdown_docs_root() {
		let root = temp_docs_root();

		fs::write(root.join("docs/index.md"), "# Docs\n").expect("index should write");
		fs::write(root.join("docs/runbook.md"), "# Runbook\n").expect("runbook should write");

		let report =
			crate::cli::docs_commands::check_docs_root(&root).expect("docs check should pass");

		assert_eq!(report.repo_root, root);
		assert_eq!(report.markdown_file_count, 2);
	}

	#[test]
	fn docs_check_requires_index() {
		let root = temp_docs_root();

		fs::write(root.join("docs/guide.md"), "# Guide\n").expect("guide should write");

		let error = crate::cli::docs_commands::check_docs_root(&root)
			.expect_err("missing docs index should fail");

		assert!(error.to_string().contains("docs/index.md"));
	}
}
