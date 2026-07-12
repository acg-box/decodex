use std::{env, fs::File, io::Read as _, process, time::{SystemTime, UNIX_EPOCH}};

use sha2::{Digest as _, Sha256};

use crate::{
	authority_broker::AuthorityBrokerSeal,
	lane_authority::{
		AccountabilityRootFingerprint, InvocationIdentity, InvocationOrigin, PrincipalKind,
		PrincipalRefNamespace, PrincipalRefToken,
	},
	prelude::Result,
};

pub(crate) fn local_process_invocation_identity(
	origin: InvocationOrigin,
	supervisor_generation: u64,
) -> Result<InvocationIdentity> {
	if supervisor_generation == 0 {
		color_eyre::eyre::bail!("Authority broker supervisor generation must be positive.");
	}
	let mut nonce = [0_u8; 32];
	File::open("/dev/urandom")?.read_exact(&mut nonce)?;
	let executable = env::current_exe()?;
	let executable_digest = Sha256::digest(fs_bytes_for_identity(&executable)?);
	let uid = unsafe { libc::getuid() };
	let principal_digest = digest_parts(&[
		b"decodex.os-audit-principal/1",
		&uid.to_be_bytes(),
		&executable_digest,
	]);
	let accountability_root = digest_parts(&[
		b"decodex.accountability-root/1",
		&principal_digest,
		&executable_digest,
	]);
	let transport_session_fingerprint = digest_parts(&[
		b"decodex.local-process-transport/1",
		&process::id().to_be_bytes(),
		&nonce,
	]);
	let authenticated_at_unix_micros = i64::try_from(
		SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(),
	)?;
	let invocation_id = format!(
		"invocation:{}",
		hex_digest(&digest_parts(&[
			b"decodex.invocation-id/1",
			&transport_session_fingerprint,
			&authenticated_at_unix_micros.to_be_bytes(),
		])),
	);
	attested_invocation_identity(
		&invocation_id,
		origin,
		PrincipalKind::OsAuditIdentity,
		PrincipalRefNamespace::OsAuditUser,
		principal_digest,
		accountability_root,
		transport_session_fingerprint,
		supervisor_generation,
		None,
		nonce,
		authenticated_at_unix_micros,
	)
}

fn fs_bytes_for_identity(path: &std::path::Path) -> Result<Vec<u8>> {
	Ok(std::fs::read(path)?)
}

fn digest_parts(parts: &[&[u8]]) -> [u8; 32] {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part);
	}
	digest.finalize().into()
}

fn hex_digest(digest: &[u8]) -> String {
	digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn local_process_identity_is_sealed_scoped_and_nonce_unique() {
		let first = local_process_invocation_identity(InvocationOrigin::Mcp, 7).expect("first");
		let second =
			local_process_invocation_identity(InvocationOrigin::Mcp, 7).expect("second");
		assert_eq!(first.origin(), InvocationOrigin::Mcp);
		assert_eq!(first.principal_kind(), PrincipalKind::OsAuditIdentity);
		assert_eq!(first.principal_ref().namespace(), PrincipalRefNamespace::OsAuditUser);
		assert_ne!(first.invocation_id(), second.invocation_id());
		assert_ne!(first.fingerprint().expect("first fingerprint"), second.fingerprint().expect("second fingerprint"));
	}
}
