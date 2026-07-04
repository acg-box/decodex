//! Execution Program graph node and edge types.

mod conflict_domain;
mod dependency;
mod node;

pub(crate) use self::{
	conflict_domain::ExecutionConflictDomain, dependency::ExecutionProgramDependency,
	node::ExecutionProgramNode,
};
