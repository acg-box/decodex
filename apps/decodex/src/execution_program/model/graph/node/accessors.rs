use crate::execution_program::{
	ExecutionConflictDomain, ExecutionLinearIssueMapping, ExecutionProgramNode,
	ExecutionProgramNodeStage, ExecutionQueueIntent,
};

impl ExecutionProgramNode {
	/// Stable internal node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Node execution stage.
	pub(crate) fn stage(&self) -> ExecutionProgramNodeStage {
		self.stage
	}

	/// Node queue intent.
	pub(crate) fn queue_intent(&self) -> ExecutionQueueIntent {
		self.queue_intent
	}

	/// Conflict domains occupied by this node.
	pub(crate) fn conflict_domains(&self) -> &[ExecutionConflictDomain] {
		&self.conflict_domains
	}

	/// Linked normal Linear issue, when the node is executable.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}

	pub(in crate::execution_program::model) fn bind_contract_fingerprint(
		&mut self,
		fingerprint: &str,
	) {
		if self.contract_fingerprint.is_empty() {
			self.contract_fingerprint = fingerprint.to_owned();
		}
	}
}
