//! Runtime control-plane CLI command definitions.

pub(in crate::cli) mod lane;
pub(in crate::cli) mod mcp;
pub(in crate::cli) mod project;

mod diagnose;
mod evidence;
mod run;
mod serve;
mod status;

pub(super) use self::{
	diagnose::DiagnoseCommand, evidence::EvidenceCommand, lane::LaneCommand, mcp::McpCommand,
	project::ProjectCommand, run::RunCommand, serve::ServeCommand, status::StatusCommand,
};
