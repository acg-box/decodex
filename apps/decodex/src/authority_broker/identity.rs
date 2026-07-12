use crate::{
	authority_broker::AuthorityBrokerSeal,
	lane_authority::{
		AccountabilityRootFingerprint, InvocationIdentity, InvocationOrigin, PrincipalKind,
		PrincipalRefNamespace, PrincipalRefToken,
	},
	prelude::Result,
};

/// Construct identity only after the broker has authenticated transport and peer facts.
#[allow(clippy::too_many_arguments)]
pub(super) fn attested_invocation_identity(
	invocation_id: &str,
	origin: InvocationOrigin,
	principal_kind: PrincipalKind,
	principal_namespace: PrincipalRefNamespace,
	principal_ref_digest: [u8; 32],
	accountability_root: [u8; 32],
	transport_session_fingerprint: [u8; 32],
	supervisor_generation: u64,
	parent_invocation_id: Option<&str>,
	nonce_digest: [u8; 32],
	authenticated_at_unix_micros: i64,
) -> Result<InvocationIdentity> {
	let seal = AuthorityBrokerSeal::new();
	InvocationIdentity::from_attested_parts(
		&seal,
		invocation_id,
		origin,
		principal_kind,
		PrincipalRefToken::from_attested_parts(&seal, principal_namespace, principal_ref_digest),
		AccountabilityRootFingerprint::from_attested_digest(&seal, accountability_root),
		transport_session_fingerprint,
		supervisor_generation,
		parent_invocation_id,
		nonce_digest,
		authenticated_at_unix_micros,
	)
}

#[cfg(test)]
pub(crate) fn test_invocation_identity() -> InvocationIdentity {
	attested_invocation_identity(
		"invocation-1",
		InvocationOrigin::Supervisor,
		PrincipalKind::ProviderAccount,
		PrincipalRefNamespace::GithubAccount,
		[1; 32],
		[2; 32],
		[3; 32],
		1,
		None,
		[4; 32],
		1,
	)
	.expect("test invocation")
}
