use std::{
	env, fs,
	path::{Component, Path, PathBuf},
};

use clap::{Args, Subcommand};

use crate::prelude::{Result, eyre};

/// OpenWiki validation commands.
#[derive(Debug, Args)]
pub(super) struct OpenWikiCommand {
	#[command(subcommand)]
	pub(super) command: OpenWikiSubcommand,
}
impl OpenWikiCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			OpenWikiSubcommand::Check(args) => args.run(),
		}
	}
}

/// OpenWiki validation subcommands.
#[derive(Debug, Subcommand)]
pub(super) enum OpenWikiSubcommand {
	/// Check the current repository OpenWiki surface.
	Check(OpenWikiCheckCommand),
}

/// Validate repository OpenWiki readiness.
#[derive(Debug, Args)]
pub(super) struct OpenWikiCheckCommand {
	/// Repository root to check. Defaults to the nearest parent with OpenWiki.
	#[arg(long, value_name = "REPO_ROOT")]
	root: Option<PathBuf>,
}
impl OpenWikiCheckCommand {
	fn run(&self) -> Result<()> {
		let root = match self.root.as_deref() {
			Some(root) => root.to_path_buf(),
			None => discover_repo_root(&env::current_dir()?)?,
		};
		let report = check_openwiki_surface(&root)?;

		println!(
			"OpenWiki surface is ready: checked {} Markdown files in {}.",
			report.markdown_files, report.surface_name
		);

		Ok(())
	}
}

#[derive(Debug)]
struct OpenWikiCheckReport {
	markdown_files: usize,
	surface_name: String,
}

fn discover_repo_root(start: &Path) -> Result<PathBuf> {
	for candidate in start.ancestors() {
		if candidate.join("openwiki").is_dir() || candidate.join(".git").exists() {
			return Ok(candidate.to_path_buf());
		}
	}

	eyre::bail!(
		"Failed to find a repository root with an OpenWiki surface from `{}`.",
		start.display()
	)
}

fn check_openwiki_surface(root: &Path) -> Result<OpenWikiCheckReport> {
	let openwiki = root.join("openwiki");
	if !openwiki.is_dir() {
		eyre::bail!("No OpenWiki surface found under `{}`. Expected `openwiki/`.", root.display());
	}

	check_openwiki_router(&openwiki)?;
	check_local_markdown_links(&openwiki)?;
	let markdown_files = count_readable_markdown_files(&openwiki)?;

	if markdown_files == 0 {
		eyre::bail!(
			"No Markdown files found under `{}`. Expected at least one checked-in OpenWiki page.",
			openwiki.display()
		);
	}

	Ok(OpenWikiCheckReport { markdown_files, surface_name: String::from("openwiki") })
}

fn check_openwiki_router(path: &Path) -> Result<()> {
	reject_openwiki_symlink(path)?;

	let quickstart = path.join("quickstart.md");
	if !quickstart.is_file() {
		eyre::bail!("OpenWiki surface is missing `{}`.", quickstart.display());
	}

	if fs::read_to_string(&quickstart)?.trim().is_empty() {
		eyre::bail!("OpenWiki router `{}` is empty.", quickstart.display());
	}

	Ok(())
}

fn check_local_markdown_links(root: &Path) -> Result<()> {
	let surface_root = root.canonicalize()?;
	for path in markdown_files(root)? {
		let text = fs::read_to_string(&path)?;
		for raw_target in markdown_link_targets(&text) {
			let Some(target) = local_markdown_link_target(&path, raw_target)? else {
				continue;
			};
			if !target.starts_with(&surface_root) {
				eyre::bail!(
					"OpenWiki link in `{}` escapes `{}`: `{}`.",
					path.display(),
					root.display(),
					raw_target
				);
			}
			let Ok(metadata) = fs::symlink_metadata(&target) else {
				eyre::bail!(
					"OpenWiki link in `{}` points to missing file `{}`.",
					path.display(),
					raw_target
				);
			};
			if metadata.file_type().is_symlink() {
				eyre::bail!(
					"OpenWiki link in `{}` points to symlink `{}`.",
					path.display(),
					raw_target
				);
			}
			if !metadata.is_file() {
				eyre::bail!(
					"OpenWiki link in `{}` points to non-file `{}`.",
					path.display(),
					raw_target
				);
			}
		}
	}

	Ok(())
}

fn reject_openwiki_symlink(path: &Path) -> Result<()> {
	if fs::symlink_metadata(path)?.file_type().is_symlink() {
		eyre::bail!("OpenWiki path must not be a symlink: `{}`.", path.display());
	}

	Ok(())
}

fn count_readable_markdown_files(path: &Path) -> Result<usize> {
	let files = markdown_files(path)?;
	for file in &files {
		if fs::read_to_string(file)?.trim().is_empty() {
			eyre::bail!("OpenWiki file `{}` is empty.", file.display());
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
			eyre::bail!("OpenWiki tree must not contain symlink `{}`.", path.display());
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
	fn openwiki_check_rejects_missing_surface() {
		let root = temp_root("missing_surface");

		let error = check_openwiki_surface(&root).expect_err("missing surface should fail");

		assert!(error.to_string().contains("No OpenWiki surface found"));
	}

	#[test]
	fn openwiki_check_rejects_missing_local_markdown_link() {
		let root = temp_root("missing_link");
		let openwiki = root.join("openwiki");
		fs::create_dir_all(&openwiki).unwrap();
		fs::write(openwiki.join("quickstart.md"), "[Missing](missing.md)\n").unwrap();

		let error = check_openwiki_surface(&root).expect_err("missing link should fail");

		assert!(error.to_string().contains("points to missing file"));
	}

	#[test]
	fn openwiki_check_rejects_empty_markdown_file() {
		let root = temp_root("empty_markdown");
		let openwiki = root.join("openwiki");
		fs::create_dir_all(&openwiki).unwrap();
		fs::write(openwiki.join("quickstart.md"), "# Quickstart\n").unwrap();
		fs::write(openwiki.join("empty.md"), "\n").unwrap();

		let error = check_openwiki_surface(&root).expect_err("empty file should fail");

		assert!(error.to_string().contains("is empty"));
	}

	#[cfg(unix)]
	#[test]
	fn openwiki_check_rejects_symlinked_entries() {
		let root = temp_root("symlinked_entry");
		let openwiki = root.join("openwiki");
		let external = root.join("external");
		fs::create_dir_all(&openwiki).unwrap();
		fs::create_dir_all(&external).unwrap();
		fs::write(openwiki.join("quickstart.md"), "# Quickstart\n").unwrap();
		fs::write(external.join("outside.md"), "# Outside\n").unwrap();
		std::os::unix::fs::symlink(&external, openwiki.join("linked")).unwrap();

		let error = check_openwiki_surface(&root).expect_err("symlinked entry should fail");

		assert!(error.to_string().contains("must not contain symlink"));
	}

	fn temp_root(name: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = env::temp_dir().join(format!("decodex_openwiki_check_{name}_{nonce}"));
		fs::create_dir_all(&root).unwrap();
		root
	}
}
