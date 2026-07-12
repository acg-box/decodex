use std::collections::BTreeMap;

use crate::{
	prelude::{Result, eyre},
	tracker::identity::TrackerProvider,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct TrackerCredentialAttestation {
	credential_ref: String,
	provider: TrackerProvider,
	account_id: String,
	workspace_id: String,
	capability_fingerprint: String,
}
#[cfg_attr(not(test), allow(dead_code))]
impl TrackerCredentialAttestation {
	pub(crate) fn linear(
		credential_ref: &str,
		account_id: &str,
		workspace_id: &str,
		capability_fingerprint: &str,
	) -> Result<Self> {
		Self::new(
			credential_ref,
			TrackerProvider::Linear,
			account_id,
			workspace_id,
			capability_fingerprint,
		)
	}

	pub(crate) fn new(
		credential_ref: &str,
		provider: TrackerProvider,
		account_id: &str,
		workspace_id: &str,
		capability_fingerprint: &str,
	) -> Result<Self> {
		for (field, value) in [
			("credential_ref", credential_ref),
			("account_id", account_id),
			("workspace_id", workspace_id),
			("capability_fingerprint", capability_fingerprint),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Tracker credential attestation `{field}` cannot be empty.");
			}
		}

		Ok(Self {
			credential_ref: credential_ref.to_owned(),
			provider,
			account_id: account_id.to_owned(),
			workspace_id: workspace_id.to_owned(),
			capability_fingerprint: capability_fingerprint.to_owned(),
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct TrackerWorkspaceEntry {
	provider: TrackerProvider,
	workspace_id: String,
	account_id: String,
	capability_fingerprint: String,
	credential_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct TrackerCredentialQuarantine {
	credential_ref: String,
	reason: TrackerCredentialQuarantineReason,
	candidate_workspace_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerCredentialQuarantineReason {
	CredentialIdentityDrift,
	WorkspaceIdentityConflict,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct TrackerWorkspaceDirectory {
	epoch: u64,
	entries: BTreeMap<String, TrackerWorkspaceEntry>,
	credential_attestations: BTreeMap<String, TrackerCredentialAttestation>,
	quarantines: BTreeMap<String, TrackerCredentialQuarantine>,
}
#[cfg_attr(not(test), allow(dead_code))]
impl TrackerWorkspaceDirectory {
	pub(crate) const fn epoch(&self) -> u64 {
		self.epoch
	}

	pub(crate) fn quarantine_count(&self) -> usize {
		self.quarantines.len()
	}

	pub(crate) fn publish(
		&mut self,
		attestation: TrackerCredentialAttestation,
	) -> Result<&TrackerWorkspaceEntry> {
		if self.quarantines.contains_key(&attestation.credential_ref) {
			eyre::bail!("Tracker credential is quarantined and cannot publish a workspace.");
		}
		if let Some(existing) = self.credential_attestations.get(&attestation.credential_ref) {
			if existing == &attestation {
				return self.entry_for(&attestation);
			}

			self.quarantine(
				&attestation,
				TrackerCredentialQuarantineReason::CredentialIdentityDrift,
				existing.workspace_id.clone(),
			);
			eyre::bail!("Tracker credential immutable identity changed during introspection.");
		}

		let key = workspace_key(attestation.provider, &attestation.workspace_id);
		if let Some(existing) = self.entries.get(&key)
			&& (existing.account_id != attestation.account_id
				|| existing.capability_fingerprint != attestation.capability_fingerprint)
		{
			let existing_workspace = existing.workspace_id.clone();
			self.quarantine(
				&attestation,
				TrackerCredentialQuarantineReason::WorkspaceIdentityConflict,
				existing_workspace,
			);
			eyre::bail!("Tracker credentials disagree on immutable workspace authority.");
		}

		self.credential_attestations
			.insert(attestation.credential_ref.clone(), attestation.clone());
		let entry = self.entries.entry(key).or_insert_with(|| TrackerWorkspaceEntry {
			provider: attestation.provider,
			workspace_id: attestation.workspace_id.clone(),
			account_id: attestation.account_id.clone(),
			capability_fingerprint: attestation.capability_fingerprint.clone(),
			credential_refs: Vec::new(),
		});
		entry.credential_refs.push(attestation.credential_ref);
		entry.credential_refs.sort();
		entry.credential_refs.dedup();
		self.epoch = self
			.epoch
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Workspace directory epoch overflow."))?;

		Ok(entry)
	}

	fn entry_for(
		&self,
		attestation: &TrackerCredentialAttestation,
	) -> Result<&TrackerWorkspaceEntry> {
		self.entries
			.get(&workspace_key(attestation.provider, &attestation.workspace_id))
			.ok_or_else(|| eyre::eyre!("Credential attestation exists without workspace entry."))
	}

	fn quarantine(
		&mut self,
		attestation: &TrackerCredentialAttestation,
		reason: TrackerCredentialQuarantineReason,
		other_workspace_id: String,
	) {
		let mut candidate_workspace_ids =
			vec![other_workspace_id, attestation.workspace_id.clone()];
		candidate_workspace_ids.sort();
		candidate_workspace_ids.dedup();
		self.quarantines.insert(
			attestation.credential_ref.clone(),
			TrackerCredentialQuarantine {
				credential_ref: attestation.credential_ref.clone(),
				reason,
				candidate_workspace_ids,
			},
		);
	}
}

fn workspace_key(provider: TrackerProvider, workspace_id: &str) -> String {
	format!("{}:{workspace_id}", provider.as_str())
}

#[cfg(test)]
mod tests {
	use super::{
		TrackerCredentialAttestation, TrackerCredentialQuarantineReason, TrackerWorkspaceDirectory,
	};
	use crate::tracker::identity::TrackerProvider;

	fn attestation(
		reference: &str,
		account: &str,
		workspace: &str,
		capability: &str,
	) -> TrackerCredentialAttestation {
		TrackerCredentialAttestation::new(
			reference,
			TrackerProvider::Linear,
			account,
			workspace,
			capability,
		)
		.expect("attestation")
	}

	#[test]
	fn duplicate_introspection_is_idempotent() {
		let mut directory = TrackerWorkspaceDirectory::default();
		let input = attestation("credential-1", "account-1", "workspace-1", "capability-1");

		directory.publish(input.clone()).expect("first publish");
		directory.publish(input).expect("duplicate publish");

		assert_eq!(directory.epoch, 1);
		assert!(directory.quarantines.is_empty());
	}

	#[test]
	fn conflicting_workspace_introspection_quarantines_before_routing() {
		let mut directory = TrackerWorkspaceDirectory::default();
		directory
			.publish(attestation("credential-1", "account-1", "workspace-1", "capability-1"))
			.expect("first credential");

		let error = directory
			.publish(attestation("credential-2", "account-conflict", "workspace-1", "capability-1"))
			.expect_err("conflicting identity must reject");

		assert!(error.to_string().contains("disagree"));
		assert_eq!(
			directory.quarantines["credential-2"].reason,
			TrackerCredentialQuarantineReason::WorkspaceIdentityConflict
		);
		assert_eq!(directory.epoch, 1);
	}
}
