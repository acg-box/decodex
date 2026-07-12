use std::env;

use crate::{
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{TrackerWorkspacePublishOutcome, linear::LinearClient},
};

/// Introspect and publish every host credential before unbound issue resolution.
pub(crate) fn bootstrap_tracker_workspace_directory(store: &StateStore) -> Result<()> {
	let catalog = runtime::tracker_credential_catalog()?;
	if catalog.is_empty() {
		eyre::bail!(
			"Host tracker credential catalog is empty; unbound issue resolution is disabled."
		);
	}
	for credential in catalog {
		let token = env::var(&credential.api_key_env_var).map_err(|_| {
			eyre::eyre!(
				"Tracker credential `{}` requires environment variable `{}`.",
				credential.credential_ref,
				credential.api_key_env_var
			)
		})?;
		if token.trim().is_empty() {
			eyre::bail!("Tracker credential token cannot be empty.");
		}
		let attestation =
			LinearClient::new(token)?.introspect_workspace_identity(&credential.credential_ref)?;
		if let TrackerWorkspacePublishOutcome::Quarantined(_) =
			store.publish_tracker_credential_attestation(attestation)?
		{
			eyre::bail!(
				"Tracker credential `{}` conflicts with workspace authority and was quarantined.",
				credential.credential_ref
			);
		}
	}

	Ok(())
}
