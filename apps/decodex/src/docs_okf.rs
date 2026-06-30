//! OKF-style documentation validation for the repository docs bundle.

use std::{
	collections::BTreeSet,
	fs,
	io::ErrorKind,
	path::{Component, Path, PathBuf},
};

use regex::Regex;
use reqwest::Url;
use serde_yaml::{self, Mapping};
use time::{Date, Month};

use crate::prelude::Result;

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
	model::{
		DocsCheckReport, DocsCheckScope, OkfBrokenLink, OkfCheckProfile, OkfCheckReport,
		OkfConceptSummary, OkfGraph, OkfGraphEdge, OkfInitReport, OkfQuery,
	},
};

use self::{
	model::{
		ALLOWED_AUTHORITIES, ALLOWED_CONCEPT_TYPES, ALLOWED_PROMOTION_TARGETS, ALLOWED_STATUSES,
		DRIFT_AUDIT_HEADINGS, DocsCheckIssue, DocsFile, OkfScaffoldFile, REQUIRED_CONCEPT_KEYS,
		REQUIRED_DOCS_FILES, RESEARCH_CONTRACT_HEADINGS,
	},
	support::{
		collect_files, concept_type, docs_dirs_with_content, file_path_set, frontmatter_string,
		frontmatter_value, is_concept_markdown, is_http_url, is_markdown,
		is_normalized_relative_path, is_valid_iso_date, issue, normalize_path, read_okf_files,
		resolve_link_target, should_skip_link_target, split_yaml_frontmatter, strip_fragment,
	},
};
