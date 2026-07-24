//! Typed vNext wire contracts and same-UID local transport shared by clients and
//! `decodexd`.

mod client;
mod doctor;
mod local_transport;
mod retained_session;
mod wire;

pub use self::{
	client::{ClientFailure, ClientProfile, DoctorClient, ProfileKind},
	doctor::{
		AppServerCapability, DoctorCheck, DoctorComponent, DoctorContractError, DoctorIssue,
		DoctorReport, DoctorStatus, MAX_DOCTOR_CHECKS,
	},
	local_transport::{
		LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
		LocalTransportStream,
	},
	retained_session::{
		ApplicationConfirmation, RetainedSession, RetainedSessionConfig, RetainedSessionFailure,
		SessionCancellation, SessionCheckpoint, SessionDelivery,
	},
	wire::{
		CausationId, Channel, ClientCommandId, ClientHello, ClientMessage, CommandEnvelope,
		CommandError, CommandOutcome, CommandPayload, CommandReceipt, CommandResultEnvelope,
		ConversationHistoryPage, ConversationHistoryResult, CorrelationId, Cursor, EntityId,
		EntityRevision, EventEnvelope, EventPayload, ExecutionConsumerDto, ExecutionDecisionDto,
		ExecutionDecisionQueryError, ExecutionDecisionResult, ExecutionQuotaExclusionDto,
		ExecutionQuotaWindowDto, ExecutionRouteBlockerDto, ExecutionRouteCauseDto,
		ExecutionRouteDto, HistoryArtifactId, HistoryArtifactReference, HistoryArtifactRevision,
		HistoryBlobLength, HistoryBlobReference, HistoryCursorToken, HistoryItemDto,
		HistoryItemKindDto, HistoryItemStatusDto, HistoryMediaType, HistoryMetadata,
		HistoryMetadataValue, HistoryPayloadDto, HistoryQueryError, HistorySideEffectState,
		HistoryText, HistoryTurnRole, IdempotencyKey,
		MAX_HISTORY_INLINE_BYTES, MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_HISTORY_PAGE_SIZE, MAX_WIRE_TEXT_BYTES,
		QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload,
		ReceiptDisposition, ReconnectMode, Refusal, RefusalEnvelope, ResultPayload, ResumeCursor,
		ServerId, ServerInstanceId, ServerMessage, ServerWelcome, Sha256Digest, SnapshotEnvelope,
		SnapshotItem, WireScalarTooLong, WireText, decode_client_message, encode_server_message,
	},
};

use serde::{Deserialize, Serialize};

use decodex_core::FoundationStatus;

/// Current protocol generation and minor revision.
pub const CURRENT_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 2 };
/// Oldest protocol revision accepted during a rolling client/server update.
pub const PREVIOUS_MINOR_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 1 };

/// A version of the Decodex application protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ProtocolVersion {
	/// Breaking protocol generation.
	pub major: u16,
	/// Compatible protocol revision within a generation.
	pub minor: u16,
}
impl ProtocolVersion {
	/// Negotiate this client version against the server's exact supported window.
	pub fn negotiate(self) -> Result<Self, VersionRefusal> {
		if self.major != CURRENT_VERSION.major {
			return Err(VersionRefusal::MajorMismatch {
				requested: self,
				supported: SupportedVersions::current(),
			});
		}
		if !(PREVIOUS_MINOR_VERSION.minor..=CURRENT_VERSION.minor).contains(&self.minor) {
			return Err(VersionRefusal::UnsupportedMinor {
				requested: self,
				supported: SupportedVersions::current(),
			});
		}

		Ok(self)
	}
}

/// The server's contiguous compatibility window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SupportedVersions {
	/// Required protocol generation.
	pub major: u16,
	/// Oldest accepted minor revision.
	pub minimum_minor: u16,
	/// Newest accepted minor revision.
	pub maximum_minor: u16,
}
impl SupportedVersions {
	/// Return the compatibility window implemented by this build.
	pub const fn current() -> Self {
		Self {
			major: CURRENT_VERSION.major,
			minimum_minor: PREVIOUS_MINOR_VERSION.minor,
			maximum_minor: CURRENT_VERSION.minor,
		}
	}
}

/// The compile-time service announcement used before a socket is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceAnnouncement {
	/// Application protocol version selected by the service.
	pub version: ProtocolVersion,
	/// Current authority-bearing adapter status.
	pub foundation: FoundationStatus,
}

/// A precise version-negotiation refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum VersionRefusal {
	/// The breaking generations differ.
	MajorMismatch {
		/// Version requested by the client.
		requested: ProtocolVersion,
		/// Versions supported by the server.
		supported: SupportedVersions,
	},
	/// The generation matches but the minor is outside the rolling window.
	UnsupportedMinor {
		/// Version requested by the client.
		requested: ProtocolVersion,
		/// Versions supported by the server.
		supported: SupportedVersions,
	},
}

#[cfg(test)]
mod tests {
	use std::net::{IpAddr, Ipv4Addr, SocketAddr};

	use crate::{
		CURRENT_VERSION, LoopbackEndpoint, PREVIOUS_MINOR_VERSION, ProtocolVersion,
		SupportedVersions, VersionRefusal,
	};

	#[test]
	fn current_and_previous_minor_versions_are_accepted_exactly() {
		assert_eq!(CURRENT_VERSION.negotiate(), Ok(CURRENT_VERSION));
		assert_eq!(PREVIOUS_MINOR_VERSION.negotiate(), Ok(PREVIOUS_MINOR_VERSION));
		assert!(matches!(
			ProtocolVersion { major: 1, minor: 3 }.negotiate(),
			Err(VersionRefusal::UnsupportedMinor { .. })
		));
	}

	#[test]
	fn a_major_mismatch_is_distinct_from_minor_incompatibility() {
		let requested = ProtocolVersion { major: 2, minor: 0 };

		assert_eq!(
			requested.negotiate(),
			Err(VersionRefusal::MajorMismatch {
				requested,
				supported: SupportedVersions::current(),
			})
		);
	}

	#[test]
	fn loopback_endpoint_accepts_local_composition() {
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
