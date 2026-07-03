//! OKF-style documentation validation for the repository docs bundle.

mod check;
mod graph;
mod init;
mod model;
mod support;
#[cfg(test)]
mod tests;

pub(crate) use self::{
	check::{render_docs_check_report, render_okf_check_report, run_docs_check, run_okf_check},
	graph::{build_okf_graph, query_okf_bundle, render_okf_graph_json, render_okf_graph_summary},
	init::{init_okf_bundle, render_okf_init_report},
	model::{DocsCheckScope, OkfCheckProfile, OkfQuery},
};
