//! Sealed accountable identity accepted by authority mutations.

use minicbor::{Decode, Encode};
use sha2::{Digest, Sha256};

use crate::{
	authority_broker::AuthorityBrokerSeal,
	prelude::{Result, eyre},
};

const INVOCATION_IDENTITY_DOMAIN: &[u8] = b"decodex.invocation-identity/1";

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(index_only)]
pub enum InvocationOrigin {
	#[n(0)]
	LocalCli,
	#[n(1)]
	LocalApp,
	#[n(2)]
	Mcp,
	#[n(3)]
	Automation,
	#[n(4)]
	Supervisor,
	#[n(5)]
	Maintenance,
}

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(index_only)]
pub enum PrincipalKind {
	#[n(0)]
	ProviderAccount,
	#[n(1)]
	OsAuditIdentity,
	#[n(2)]
	AutomationService,
}

#[derive(Clone, Copy, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(index_only)]
pub enum PrincipalRefNamespace {
	#[n(0)]
	GithubAccount,
	#[n(1)]
	LinearAccount,
	#[n(2)]
	OsAuditUser,
	#[n(3)]
	AutomationIdentity,
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub struct PrincipalRefToken {
	#[n(0)]
	namespace: PrincipalRefNamespace,
	#[n(1)]
	#[cbor(with = "minicbor::bytes")]
	digest: Vec<u8>,
}
impl PrincipalRefToken {
	pub(crate) fn from_attested_parts(
		_seal: &AuthorityBrokerSeal,
		namespace: PrincipalRefNamespace,
		digest: [u8; 32],
	) -> Self {
		Self { namespace, digest: digest.to_vec() }
	}

	pub const fn namespace(&self) -> PrincipalRefNamespace {
		self.namespace
	}

	pub fn digest(&self) -> &[u8] {
		&self.digest
	}
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(transparent)]
pub struct AccountabilityRootFingerprint(
	#[n(0)]
	#[cbor(with = "minicbor::bytes")]
	Vec<u8>,
);
impl AccountabilityRootFingerprint {
	pub(crate) fn from_attested_digest(_seal: &AuthorityBrokerSeal, digest: [u8; 32]) -> Self {
		Self(digest.to_vec())
	}

	pub fn digest(&self) -> &[u8] {
		&self.0
	}
}

#[derive(Clone, Debug, Decode, Encode, Eq, PartialEq)]
#[cbor(map)]
pub struct InvocationIdentity {
	#[n(0)]
	invocation_id: String,
	#[n(1)]
	origin: InvocationOrigin,
	#[n(2)]
	principal_kind: PrincipalKind,
	#[n(3)]
	principal_ref: PrincipalRefToken,
	#[n(4)]
	accountability_root: AccountabilityRootFingerprint,
	#[n(5)]
	#[cbor(with = "minicbor::bytes")]
	transport_session_fingerprint: Vec<u8>,
	#[n(6)]
	supervisor_generation: u64,
	#[n(7)]
	parent_invocation_id: Option<String>,
	#[n(8)]
	#[cbor(with = "minicbor::bytes")]
	nonce_digest: Vec<u8>,
	#[n(9)]
	authenticated_at_unix_micros: i64,
}
impl InvocationIdentity {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn from_attested_parts(
		_seal: &AuthorityBrokerSeal,
		invocation_id: &str,
		origin: InvocationOrigin,
		principal_kind: PrincipalKind,
		principal_ref: PrincipalRefToken,
		accountability_root: AccountabilityRootFingerprint,
		transport_session_fingerprint: [u8; 32],
		supervisor_generation: u64,
		parent_invocation_id: Option<&str>,
		nonce_digest: [u8; 32],
		authenticated_at_unix_micros: i64,
	) -> Result<Self> {
		let identity = Self {
			invocation_id: invocation_id.to_owned(),
			origin,
			principal_kind,
			principal_ref,
			accountability_root,
			transport_session_fingerprint: transport_session_fingerprint.to_vec(),
			supervisor_generation,
			parent_invocation_id: parent_invocation_id.map(ToOwned::to_owned),
			nonce_digest: nonce_digest.to_vec(),
			authenticated_at_unix_micros,
		};
		identity.validate()?;
		Ok(identity)
	}

	pub fn invocation_id(&self) -> &str {
		&self.invocation_id
	}

	pub const fn origin(&self) -> InvocationOrigin {
		self.origin
	}

	pub const fn principal_kind(&self) -> PrincipalKind {
		self.principal_kind
	}

	pub fn principal_ref(&self) -> &PrincipalRefToken {
		&self.principal_ref
	}

	pub fn accountability_root(&self) -> &AccountabilityRootFingerprint {
		&self.accountability_root
	}

	pub fn fingerprint(&self) -> Result<[u8; 32]> {
		let mut digest = Sha256::new();
		digest.update(INVOCATION_IDENTITY_DOMAIN);
		digest.update(minicbor::to_vec(self)?);
		Ok(digest.finalize().into())
	}

	fn validate(&self) -> Result<()> {
		if self.invocation_id.trim().is_empty()
			|| self.invocation_id.len() > 128
			|| self
				.parent_invocation_id
				.as_ref()
				.is_some_and(|id| id.trim().is_empty() || id.len() > 128)
			|| self.parent_invocation_id.as_deref() == Some(self.invocation_id.as_str())
			|| self.principal_ref.digest.len() != 32
			|| self.accountability_root.0.len() != 32
			|| self.transport_session_fingerprint.len() != 32
			|| self.nonce_digest.len() != 32
			|| self.supervisor_generation == 0
		{
			eyre::bail!("Invocation identity attestation is invalid.");
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn lane_authority_v2_c5_invocation_identity_is_stable_and_opaque() {
		let identity = crate::authority_broker::test_invocation_identity();
		assert_eq!(
			identity.fingerprint().expect("fingerprint"),
			identity.fingerprint().expect("stable")
		);
		let encoded = minicbor::to_vec(&identity).expect("encode");
		let text = String::from_utf8_lossy(&encoded);
		for forbidden in ["github.com", "@", "Bearer", "/Users/", "session-token"] {
			assert!(!text.contains(forbidden));
		}
		assert_eq!(identity.principal_ref().digest().len(), 32);
		assert_eq!(identity.accountability_root().digest().len(), 32);
	}
}
