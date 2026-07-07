mod context;
mod server;
mod templates;
mod types;

#[cfg(test)]
pub(super) use types::ResourceContent;

const DOCS_HOST: &str = "docs";
const RESEARCH_HOST: &str = "research";
const DECISION_CONTRACTS_HOST: &str = "decision-contracts";
const PROJECTS_HOST: &str = "projects";
