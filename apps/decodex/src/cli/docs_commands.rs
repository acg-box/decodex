use std::{
	env, fs,
	path::{Component, Path, PathBuf},
};

use clap::{Args, Subcommand};

use crate::prelude::{Result, eyre};

/// Documentation validation commands.
#[derive(Debug, Args)]
pub(super) struct DocsCommand {
	#[command(subcommand)]
	pub(super) command: DocsSubcommand,
}
impl DocsCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			DocsSubcommand::Check(args) => args.run(),
		}
	}
}

/// Documentation validation subcommands.
#[derive(Debug, Subcommand)]
pub(super) enum DocsSubcommand {
	/// Check the current repository documentation surface.
	Check(DocsCheckCommand),
}

/// Validate repository documentation readiness.
#[derive(Debug, Args)]
pub(super) struct DocsCheckCommand {
	/// Repository root to check. Defaults to the nearest parent with docs or OpenWiki.
	#[arg(long, value_name = "REPO_ROOT")]
	root: Option<PathBuf>,
}
impl DocsCheckCommand {
	fn run(&self) -> Result<()> {
		let root = match self.root.as_deref() {
			Some(root) => root.to_path_buf(),
			None => discover_repo_root(&env::current_dir()?)?,
		};
		let report = check_docs_surface(&root)?;

		println!(
			"Documentation surface is ready: checked {} Markdown files in {}.",
			report.markdown_files,
			report.surface_names.join(", ")
		);

		Ok(())
	}
}

#[derive(Debug)]
struct DocsCheckReport {
	markdown_files: usize,
	surface_names: Vec<String>,
}

const PUBFI_DOCS_ENTRYPOINTS: &[&str] = &[
	"index.md",
	"policy.md",
	"log.md",
	"spec/index.md",
	"runbook/index.md",
	"reference/index.md",
	"decisions/index.md",
	"research/index.md",
	"evidence/index.md",
];

const PUBFI_RETIRED_DOCS_LANES: &[&str] = &["prod_spec"];

fn discover_repo_root(start: &Path) -> Result<PathBuf> {
	for candidate in start.ancestors() {
		if candidate.join("openwiki").is_dir()
			|| candidate.join("docs").is_dir()
			|| candidate.join(".git").exists()
		{
			return Ok(candidate.to_path_buf());
		}
	}

	eyre::bail!(
		"Failed to find a repository root with an OpenWiki or docs surface from `{}`.",
		start.display()
	)
}

fn check_docs_surface(root: &Path) -> Result<DocsCheckReport> {
	let surfaces = docs_surfaces(root);
	if surfaces.is_empty() {
		eyre::bail!(
			"No documentation surface found under `{}`. Expected `openwiki/` or `docs/`.",
			root.display()
		);
	}

	let mut markdown_files = 0;
	let mut surface_names = Vec::new();

	for surface in surfaces {
		surface.check()?;
		markdown_files += count_readable_markdown_files(&surface.path)?;
		surface_names.push(surface.name);
	}

	if markdown_files == 0 {
		eyre::bail!(
			"No Markdown files found under `{}`. Expected at least one checked-in docs page.",
			root.display()
		);
	}

	Ok(DocsCheckReport { markdown_files, surface_names })
}

fn docs_surfaces(root: &Path) -> Vec<DocsSurface> {
	let mut surfaces = Vec::new();
	let openwiki = root.join("openwiki");
	if openwiki.is_dir() {
		surfaces.push(DocsSurface { name: String::from("openwiki"), path: openwiki });
	}
	let docs = root.join("docs");
	if docs.is_dir() {
		surfaces.push(DocsSurface { name: String::from("docs"), path: docs });
	}

	surfaces
}

struct DocsSurface {
	name: String,
	path: PathBuf,
}
impl DocsSurface {
	fn check(&self) -> Result<()> {
		reject_docs_symlink(&self.path)?;
		if self.name == "openwiki" {
			self.check_openwiki_router()?;
			self.check_local_markdown_links()?;
		}
		if self.name == "docs" {
			self.check_pubfi_docs_taxonomy_when_present()?;
			self.check_local_markdown_links()?;
		}

		Ok(())
	}

	fn check_openwiki_router(&self) -> Result<()> {
		let quickstart = self.path.join("quickstart.md");
		if !quickstart.is_file() {
			eyre::bail!("OpenWiki surface is missing `{}`.", quickstart.display());
		}

		if fs::read_to_string(&quickstart)?.trim().is_empty() {
			eyre::bail!("OpenWiki router `{}` is empty.", quickstart.display());
		}

		Ok(())
	}

	fn check_pubfi_docs_taxonomy_when_present(&self) -> Result<()> {
		if !self.path.join("policy.md").exists() {
			return Ok(());
		}

		for relative in PUBFI_DOCS_ENTRYPOINTS {
			let path = self.path.join(relative);
			if !path.is_file() {
				eyre::bail!("Docs surface is missing required entrypoint `{}`.", path.display());
			}
		}

		for relative in PUBFI_RETIRED_DOCS_LANES {
			let path = self.path.join(relative);
			if path.exists() {
				eyre::bail!("Retired docs lane must not exist: `{}`.", path.display());
			}
		}

		Ok(())
	}

	fn check_local_markdown_links(&self) -> Result<()> {
		let surface_root = self.path.canonicalize()?;
		for path in markdown_files(&self.path)? {
			let text = fs::read_to_string(&path)?;
			for raw_target in markdown_link_targets(&text) {
				let Some(target) = local_markdown_link_target(&path, raw_target)? else {
					continue;
				};
				if !target.starts_with(&surface_root) {
					eyre::bail!(
						"Documentation link in `{}` escapes `{}`: `{}`.",
						path.display(),
						self.path.display(),
						raw_target
					);
				}
				let Ok(metadata) = fs::symlink_metadata(&target) else {
					eyre::bail!(
						"Documentation link in `{}` points to missing file `{}`.",
						path.display(),
						raw_target
					);
				};
				if metadata.file_type().is_symlink() {
					eyre::bail!(
						"Documentation link in `{}` points to symlink `{}`.",
						path.display(),
						raw_target
					);
				}
				if !metadata.is_file() {
					eyre::bail!(
						"Documentation link in `{}` points to non-file `{}`.",
						path.display(),
						raw_target
					);
				}
			}
		}

		Ok(())
	}
}

fn reject_docs_symlink(path: &Path) -> Result<()> {
	if fs::symlink_metadata(path)?.file_type().is_symlink() {
		eyre::bail!("Documentation path must not be a symlink: `{}`.", path.display());
	}

	Ok(())
}

fn count_readable_markdown_files(path: &Path) -> Result<usize> {
	let files = markdown_files(path)?;
	for file in &files {
		if fs::read_to_string(file)?.trim().is_empty() {
			eyre::bail!("Documentation file `{}` is empty.", file.display());
		}
	}

	Ok(files.len())
}

fn markdown_files(path: &Path) -> Result<Vec<PathBuf>> {
	let mut files = Vec::new();
	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;
		if file_type.is_symlink() {
			eyre::bail!("Documentation tree must not contain symlink `{}`.", path.display());
		}
		if file_type.is_dir() {
			files.extend(markdown_files(&path)?);
			continue;
		}
		if !file_type.is_file() {
			continue;
		}
		if !is_markdown_file(&path) {
			continue;
		}
		files.push(path);
	}
	Ok(files)
}

fn is_markdown_file(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| matches!(extension, "md" | "mdx"))
}

fn markdown_link_targets(text: &str) -> Vec<&str> {
	let mut targets = Vec::new();
	let mut cursor = text;
	while let Some(label_start) = cursor.find('[') {
		cursor = &cursor[label_start + 1..];
		let Some(label_end) = cursor.find(']') else {
			break;
		};
		cursor = &cursor[label_end + 1..];
		if !cursor.starts_with('(') {
			continue;
		}
		cursor = &cursor[1..];
		let Some(target_end) = cursor.find(')') else {
			break;
		};
		targets.push(cursor[..target_end].trim());
		cursor = &cursor[target_end + 1..];
	}
	targets
}

fn local_markdown_link_target(path: &Path, raw_target: &str) -> Result<Option<PathBuf>> {
	if raw_target.is_empty()
		|| raw_target.starts_with('#')
		|| raw_target.starts_with('/')
		|| raw_target.starts_with("http://")
		|| raw_target.starts_with("https://")
		|| raw_target.starts_with("mailto:")
	{
		return Ok(None);
	}
	let target = raw_target.split('#').next().unwrap_or_default();
	if target.is_empty() || !target.ends_with(".md") {
		return Ok(None);
	}
	let Some(parent) = path.parent() else {
		return Ok(None);
	};
	let mut resolved = parent.canonicalize()?;
	for component in Path::new(target).components() {
		match component {
			Component::CurDir => {},
			Component::Normal(value) => resolved.push(value),
			Component::ParentDir => {
				resolved.pop();
			},
			Component::Prefix(_) | Component::RootDir => return Ok(None),
		}
	}
	Ok(Some(resolved))
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		time::{SystemTime, UNIX_EPOCH},
	};

	use super::*;

	#[test]
	fn docs_check_rejects_missing_pubfi_entrypoint() {
		let root = temp_root("missing_pubfi_entrypoint");
		let docs = root.join("docs");
		fs::create_dir_all(&docs).unwrap();
		fs::write(docs.join("policy.md"), "# Policy\n").unwrap();

		let error = check_docs_surface(&root).expect_err("missing entrypoints should fail");

		assert!(error.to_string().contains("missing required entrypoint"));
	}

	#[test]
	fn docs_check_rejects_missing_local_markdown_link() {
		let root = temp_root("missing_link");
		let openwiki = root.join("openwiki");
		fs::create_dir_all(&openwiki).unwrap();
		fs::write(openwiki.join("quickstart.md"), "[Missing](missing.md)\n").unwrap();

		let error = check_docs_surface(&root).expect_err("missing link should fail");

		assert!(error.to_string().contains("points to missing file"));
	}

	#[test]
	fn docs_check_rejects_empty_markdown_file() {
		let root = temp_root("empty_markdown");
		let openwiki = root.join("openwiki");
		fs::create_dir_all(&openwiki).unwrap();
		fs::write(openwiki.join("quickstart.md"), "# Quickstart\n").unwrap();
		fs::write(openwiki.join("empty.md"), "\n").unwrap();

		let error = check_docs_surface(&root).expect_err("empty file should fail");

		assert!(error.to_string().contains("is empty"));
	}

	#[cfg(unix)]
	#[test]
	fn docs_check_rejects_symlinked_docs_entries() {
		let root = temp_root("symlinked_docs_entry");
		let openwiki = root.join("openwiki");
		let external = root.join("external");
		fs::create_dir_all(&openwiki).unwrap();
		fs::create_dir_all(&external).unwrap();
		fs::write(openwiki.join("quickstart.md"), "# Quickstart\n").unwrap();
		fs::write(external.join("outside.md"), "# Outside\n").unwrap();
		std::os::unix::fs::symlink(&external, openwiki.join("linked")).unwrap();

		let error = check_docs_surface(&root).expect_err("symlinked entry should fail");

		assert!(error.to_string().contains("must not contain symlink"));
	}

	fn temp_root(name: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = env::temp_dir().join(format!("decodex_docs_check_{name}_{nonce}"));
		fs::create_dir_all(&root).unwrap();
		root
	}
}
