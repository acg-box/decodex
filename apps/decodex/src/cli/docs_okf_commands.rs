//! Docs and OKF CLI command definitions.

mod profiles;
mod query;

pub(super) use self::{
	profiles::{OkfInitProfileArg, OkfProfileArg},
	query::OkfFindFilters,
};

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{
	docs_okf::{self, DocsCheckScope, OkfCheckProfile},
	prelude::{Result, eyre},
};

#[derive(Debug, Args)]
pub(super) struct DocsCommand {
	/// Documentation root to validate.
	#[arg(long, value_name = "DIR", default_value = "docs")]
	pub(super) root: PathBuf,
	#[command(subcommand)]
	pub(super) command: DocsSubcommand,
}
impl DocsCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			DocsSubcommand::Check => self.run_check(DocsCheckScope::All),
			DocsSubcommand::Index => self.run_check(DocsCheckScope::Index),
			DocsSubcommand::Links => self.run_check(DocsCheckScope::Links),
			DocsSubcommand::Drift => self.run_check(DocsCheckScope::Drift),
			DocsSubcommand::Find(args) => query::run_okf_find(&self.root, &args.filters),
			DocsSubcommand::Graph(args) => query::run_okf_graph(&self.root, args.json),
		}
	}

	fn run_check(&self, scope: DocsCheckScope) -> Result<()> {
		let report = docs_okf::run_docs_check(&self.root, scope)?;

		print!("{}", docs_okf::render_docs_check_report(&report));

		if report.has_issues() {
			eyre::bail!("docs {} check failed.", scope.as_str());
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(super) struct OkfCommand {
	#[command(subcommand)]
	pub(super) command: OkfSubcommand,
}
impl OkfCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			OkfSubcommand::Init(args) => args.run(),
			OkfSubcommand::Check(args) => args.run(),
			OkfSubcommand::Find(args) => query::run_okf_find(&args.root, &args.filters),
			OkfSubcommand::Graph(args) => query::run_okf_graph(&args.root, args.json),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct OkfInitCommand {
	/// OKF bundle root to scaffold.
	#[arg(value_name = "ROOT", default_value = "docs")]
	pub(super) root: PathBuf,
	/// Portable profile scaffold to create.
	#[arg(long, value_enum, default_value_t = OkfInitProfileArg::RepoMemory)]
	pub(super) profile: OkfInitProfileArg,
}
impl OkfInitCommand {
	fn run(&self) -> Result<()> {
		let profile = OkfCheckProfile::from(self.profile);
		let init_report = docs_okf::init_okf_bundle(&self.root, profile)?;

		print!("{}", docs_okf::render_okf_init_report(&init_report));

		let check_report = docs_okf::run_okf_check(&self.root, profile)?;

		print!("{}", docs_okf::render_okf_check_report(&check_report));

		if check_report.has_issues() {
			eyre::bail!("okf {} scaffold validation failed.", check_report.profile().as_str());
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(super) struct OkfCheckCommand {
	/// OKF bundle root.
	#[arg(value_name = "ROOT", default_value = "docs")]
	root: PathBuf,
	/// Validation profile to apply.
	#[arg(long, value_enum, default_value_t = OkfProfileArg::Core)]
	profile: OkfProfileArg,
}
impl OkfCheckCommand {
	fn run(&self) -> Result<()> {
		let profile = OkfCheckProfile::from(self.profile);
		let report = docs_okf::run_okf_check(&self.root, profile)?;

		print!("{}", docs_okf::render_okf_check_report(&report));

		if report.has_issues() {
			eyre::bail!("okf {} check failed.", report.profile().as_str());
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
pub(super) struct OkfFindCommand {
	/// OKF bundle root.
	#[arg(value_name = "ROOT", default_value = "docs")]
	pub(super) root: PathBuf,
	#[command(flatten)]
	pub(super) filters: OkfFindFilters,
}

#[derive(Debug, Args)]
pub(super) struct OkfGraphCommand {
	/// OKF bundle root.
	#[arg(value_name = "ROOT", default_value = "docs")]
	pub(super) root: PathBuf,
	/// Emit graph JSON instead of a text summary.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct DocsFindCommand {
	#[command(flatten)]
	pub(super) filters: OkfFindFilters,
}

#[derive(Debug, Args)]
pub(super) struct DocsGraphCommand {
	/// Emit graph JSON instead of a text summary.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Subcommand)]
pub(super) enum DocsSubcommand {
	/// Validate the complete Markdown-only Decodex docs bundle.
	Check,
	/// Validate OKF index files and concept frontmatter.
	Index,
	/// Validate local Markdown links.
	Links,
	/// Validate semantic-drift audit structure.
	Drift,
	/// Find concepts in the default docs bundle.
	Find(DocsFindCommand),
	/// Print the default docs bundle graph.
	Graph(DocsGraphCommand),
}

#[derive(Debug, Subcommand)]
pub(super) enum OkfSubcommand {
	/// Initialize a portable OKF bundle scaffold.
	Init(OkfInitCommand),
	/// Validate an OKF bundle with a selected profile.
	Check(OkfCheckCommand),
	/// Find concepts by frontmatter fields.
	Find(OkfFindCommand),
	/// Print an OKF concept graph.
	Graph(OkfGraphCommand),
}
