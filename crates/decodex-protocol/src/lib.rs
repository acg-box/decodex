//! Version and network-boundary contracts shared by vNext clients and `decodexd`.

use std::{
	error::Error,
	fmt::{Display, Formatter},
	net::SocketAddr,
};

use decodex_core::FoundationStatus;

/// The first vNext application-protocol version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolVersion {
	/// Breaking protocol generation.
	pub major: u16,
	/// Compatible protocol revision within a generation.
	pub minor: u16,
}
impl ProtocolVersion {
	/// Initial vNext protocol identifier.
	pub const V1: Self = Self { major: 1, minor: 0 };
}

/// A local endpoint that has passed the V1 loopback-only policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoopbackEndpoint(SocketAddr);
impl LoopbackEndpoint {
	/// Validate an address against the V1 loopback-only policy.
	pub fn new(address: SocketAddr) -> Result<Self, EndpointPolicyError> {
		if address.ip().is_loopback() {
			Ok(Self(address))
		} else {
			Err(EndpointPolicyError { address })
		}
	}

	/// Return the validated socket address.
	pub const fn address(self) -> SocketAddr {
		self.0
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Error returned when a composition root selects a non-loopback address.
pub struct EndpointPolicyError {
	address: SocketAddr,
}
impl Error for EndpointPolicyError {}

impl Display for EndpointPolicyError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "non-loopback endpoint is disabled: {}", self.address)
	}
}

/// The compile-time service announcement; no transport is implemented in XY-1265.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceAnnouncement {
	/// Application protocol version selected by the service.
	pub version: ProtocolVersion,
	/// Current authority-bearing adapter status.
	pub foundation: FoundationStatus,
}

#[cfg(test)]
mod tests {
	use std::net::{IpAddr, Ipv4Addr, SocketAddr};

	use crate::LoopbackEndpoint;

	#[test]
	fn loopback_endpoint_accepts_local_v1_composition() {
		let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152);

		assert_eq!(LoopbackEndpoint::new(address).unwrap().address(), address);
	}

	#[test]
	fn loopback_endpoint_refuses_remote_binding() {
		let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 49_152);

		assert_eq!(
			LoopbackEndpoint::new(address).unwrap_err().to_string(),
			"non-loopback endpoint is disabled: 0.0.0.0:49152"
		);
	}
}
