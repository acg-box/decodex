use std::path::Path;

use crate::research_design::ResearchDesignRunInput;

/// CLI/runtime request for compiling and persisting one research/design contract.
pub(crate) struct ResearchDesignCompileRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) input: ResearchDesignRunInput,
}

/// CLI/runtime request for promoting an already persisted research/design contract.
pub(crate) struct ResearchDesignPromoteRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) contract_id: &'a str,
	pub(crate) accepted_by: &'a str,
	pub(crate) accepted_at: Option<&'a str>,
	pub(crate) acceptance_source: &'a str,
	pub(crate) promotion_reason: Option<String>,
}
