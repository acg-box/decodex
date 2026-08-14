//! Closed RoleProfile identity used by the supported local Task slice.

/// Stable global RoleProfile identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoleProfileRole {
	Advisor,
	Lead,
	Task,
	Reviewer,
}

impl RoleProfileRole {
	pub(crate) const fn as_sql(self) -> &'static str {
		match self {
			Self::Advisor => "advisor",
			Self::Lead => "lead",
			Self::Task => "task",
			Self::Reviewer => "reviewer",
		}
	}
}
