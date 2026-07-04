use serde::{Deserialize, Serialize};

use crate::{
	execution_program::{model::state::ExecutionConflictDomainKind, validation},
	prelude::Result,
};

/// Conflict-domain key for one program node.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionConflictDomain {
	pub(in crate::execution_program) kind: ExecutionConflictDomainKind,
	key: String,
}
impl ExecutionConflictDomain {
	/// Build a conflict-domain key.
	pub(crate) fn new(kind: ExecutionConflictDomainKind, key: impl Into<String>) -> Result<Self> {
		let domain = Self { kind, key: key.into() };

		domain.validate()?;

		Ok(domain)
	}

	/// Stable conflict-domain key.
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	/// Stable conflict-domain kind.
	pub(crate) fn kind(&self) -> ExecutionConflictDomainKind {
		self.kind
	}

	pub(in crate::execution_program::model) fn validate(&self) -> Result<()> {
		validation::validate_required("execution program conflict_domain.key", &self.key)
	}
}
