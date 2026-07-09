use std::{
	env, fs,
	path::{Path, PathBuf},
};

use clap::{Args, Subcommand};

use crate::prelude::{Result, eyre};

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

#[derive(Debug, Subcommand)]
pub(super) enum DocsSubcommand {
	/// Check the current repository documentation surface.
	Check(DocsCheckCommand),
}

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

struct DocsCheckReport {
	markdown_files: usize,
	surface_names: Vec<String>,
}

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
		if self.name == "openwiki" {
			self.check_openwiki_router()?;
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
}

fn count_readable_markdown_files(path: &Path) -> Result<usize> {
	let mut count = 0;

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();
		if path.is_dir() {
			count += count_readable_markdown_files(&path)?;
			continue;
		}
		if !is_markdown_file(&path) {
			continue;
		}
		if fs::read_to_string(&path)?.trim().is_empty() {
			eyre::bail!("Documentation file `{}` is empty.", path.display());
		}
		count += 1;
	}

	Ok(count)
}

fn is_markdown_file(path: &Path) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| matches!(extension, "md" | "mdx"))
}
