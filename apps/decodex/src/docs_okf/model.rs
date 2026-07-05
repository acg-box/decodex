//! Data types and constants for OKF docs checks.

pub(in crate::docs_okf) mod constants;
pub(in crate::docs_okf) mod files;
pub(in crate::docs_okf) mod reports;

mod graph;
mod scope;

pub(crate) use self::{
	graph::{OkfBrokenLink, OkfConceptSummary, OkfGraph, OkfGraphEdge, OkfQuery},
	reports::{DocsCheckReport, OkfCheckReport, OkfInitReport},
	scope::{DocsCheckScope, OkfCheckProfile},
};
