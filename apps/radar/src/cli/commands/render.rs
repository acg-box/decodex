use std::path::PathBuf;

use clap::Args;

use crate::{RadarRenderSignalRequest, prelude::Result};

#[derive(Debug, Args)]
pub(in crate::cli) struct RadarRenderSignalCommand {
	#[arg(long, value_name = "FILE")]
	bundle: PathBuf,
	#[arg(long, value_name = "FILE")]
	analysis: PathBuf,
	#[arg(long, value_name = "FILE")]
	out: PathBuf,
	#[arg(long)]
	published_at: Option<String>,
}
impl RadarRenderSignalCommand {
	pub(super) fn run(&self) -> Result<()> {
		let report = crate::render_signal(&RadarRenderSignalRequest {
			bundle: self.bundle.clone(),
			analysis: self.analysis.clone(),
			out: self.out.clone(),
			published_at: self.published_at.clone(),
		})?;

		println!("{report:#?}");

		Ok(())
	}
}
