pub(in crate::docs_okf) const REQUIRED_CONCEPT_KEYS: &[&str] =
	&["type", "title", "description", "status", "authority", "owner", "last_verified"];
pub(in crate::docs_okf) const REQUIRED_DOCS_FILES: &[&str] = &[
	"index.md",
	"log.md",
	"policy.md",
	"decisions/index.md",
	"evidence/index.md",
	"reference/index.md",
	"research/index.md",
	"runbook/index.md",
	"spec/index.md",
];
pub(in crate::docs_okf) const ALLOWED_CONCEPT_TYPES: &[&str] = &[
	"Decision",
	"Drift Audit",
	"Evidence",
	"Policy",
	"Reference",
	"Research Contract",
	"Runbook",
	"Spec",
];
pub(in crate::docs_okf) const ALLOWED_STATUSES: &[&str] =
	&["draft", "active", "deprecated", "superseded"];
pub(in crate::docs_okf) const ALLOWED_AUTHORITIES: &[&str] =
	&["normative", "procedural", "current_state", "rationale", "evidence", "non_authoritative"];
pub(in crate::docs_okf) const ALLOWED_PROMOTION_TARGETS: &[&str] =
	&["docs/spec", "docs/runbook", "docs/reference", "docs/decisions", "docs/evidence"];
pub(in crate::docs_okf) const RESEARCH_CONTRACT_HEADINGS: &[&str] = &[
	"Question",
	"Scope",
	"Evidence",
	"Options",
	"Judgment",
	"Challenge",
	"Decision",
	"Promotion",
	"Drift Impact",
	"Citations",
];
pub(in crate::docs_okf) const DRIFT_AUDIT_HEADINGS: &[&str] = &[
	"Watched Claims",
	"Evidence Anchors",
	"Reverse Checks",
	"Verdict",
	"Required Updates",
	"Citations",
];
