//! Bounded, redacted doctor/status contract carried only by the versioned protocol.

use std::{
	collections::BTreeSet,
	error::Error,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ProtocolVersion, ServerId};

/// Maximum number of typed checks in one doctor report.
pub const MAX_DOCTOR_CHECKS: usize = 32;

/// One authoritative, server-produced doctor/status report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DoctorReport {
	server_id: ServerId,
	version: ProtocolVersion,
	checks: Vec<DoctorCheck>,
}
impl DoctorReport {
	/// Construct a bounded report with at most one check for each typed component.
	pub fn new(
		server_id: ServerId,
		version: ProtocolVersion,
		checks: Vec<DoctorCheck>,
	) -> Result<Self, DoctorContractError> {
		if checks.len() > MAX_DOCTOR_CHECKS {
			return Err(DoctorContractError::TooManyChecks);
		}

		let unique = checks.iter().map(|check| check.component).collect::<BTreeSet<_>>();

		if unique.len() != checks.len() {
			return Err(DoctorContractError::DuplicateComponent);
		}

		Ok(Self { server_id, version, checks })
	}

	/// Stable identity of the server host that produced the report.
	pub fn server_id(&self) -> &ServerId {
		&self.server_id
	}

	/// Protocol version owning this report shape.
	pub const fn version(&self) -> ProtocolVersion {
		self.version
	}

	/// Fixed, bounded typed checks. No server-host path or credential text is representable.
	pub fn checks(&self) -> &[DoctorCheck] {
		&self.checks
	}

	/// Look up one typed component without relying on report ordering.
	pub fn check(&self, component: DoctorComponent) -> Option<&DoctorCheck> {
		self.checks.iter().find(|check| check.component == component)
	}

	/// Whether this report contains exactly the complete current component set.
	///
	/// Ordering is not authoritative. Construction and decoding separately enforce
	/// boundedness and uniqueness.
	pub fn has_current_component_set(&self) -> bool {
		self.checks.len() == DoctorComponent::ALL.len()
			&& DoctorComponent::ALL.into_iter().all(|component| self.check(component).is_some())
	}
}

impl<'de> Deserialize<'de> for DoctorReport {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		struct RawDoctorReport {
			server_id: ServerId,
			version: ProtocolVersion,
			checks: Vec<DoctorCheck>,
		}

		let raw = RawDoctorReport::deserialize(deserializer)?;

		Self::new(raw.server_id, raw.version, raw.checks).map_err(D::Error::custom)
	}
}

/// A single bounded doctor observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DoctorCheck {
	/// Typed subsystem or authority boundary observed by decodexd.
	pub component: DoctorComponent,
	/// Typed readiness and failure class with no free-form external text.
	pub status: DoctorStatus,
}
impl DoctorCheck {
	/// Construct one typed check.
	pub const fn new(component: DoctorComponent, status: DoctorStatus) -> Self {
		Self { component, status }
	}
}

/// Doctor components owned by the current server slice.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum DoctorComponent {
	/// Typed operator configuration.
	Configuration,
	/// PostgreSQL product-state authority.
	ProductStore,
	/// Ordinary Quick Task execution composition.
	QuickTask,
	/// Versioned WebSocket application protocol.
	Protocol,
	/// Negotiated protocol compatibility window and current report schema.
	ProtocolVersion,
	/// Stable server-host identity.
	ServerIdentity,
	/// Shared Codex-owned home used for continuation.
	SharedCodexHome,
	/// One exact typed app-server capability.
	AppServerCapability(AppServerCapability),
	/// Optional managed-repository effect composition.
	ManagedRepository,
	/// Content-addressed blob-store integrity.
	BlobIntegrity,
	/// Host credential-vault boundary.
	CredentialVault,
	/// Required plugin inventory/readiness.
	PluginReadiness,
}
impl DoctorComponent {
	/// Complete closed component set in stable diagnostic order.
	pub const ALL: [Self; 19] = [
		Self::Configuration,
		Self::ProductStore,
		Self::QuickTask,
		Self::Protocol,
		Self::ProtocolVersion,
		Self::ServerIdentity,
		Self::SharedCodexHome,
		Self::AppServerCapability(AppServerCapability::Initialize),
		Self::AppServerCapability(AppServerCapability::AccountRead),
		Self::AppServerCapability(AppServerCapability::ThreadList),
		Self::AppServerCapability(AppServerCapability::ThreadRead),
		Self::AppServerCapability(AppServerCapability::ThreadArchive),
		Self::AppServerCapability(AppServerCapability::PaginatedHistory),
		Self::AppServerCapability(AppServerCapability::NativeCollaboration),
		Self::AppServerCapability(AppServerCapability::ThreadSearch),
		Self::ManagedRepository,
		Self::BlobIntegrity,
		Self::CredentialVault,
		Self::PluginReadiness,
	];
}

/// App-server capabilities exposed without raw method or schema text.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerCapability {
	/// JSON-RPC initialization handshake.
	Initialize,
	/// Active-account identity readback.
	AccountRead,
	/// Bounded thread listing.
	ThreadList,
	/// Exact-ID thread readback.
	ThreadRead,
	/// Explicit thread archival.
	ThreadArchive,
	/// Paginated persisted history.
	PaginatedHistory,
	/// Native run-local collaboration events.
	NativeCollaboration,
	/// Read-only thread search without a global-discovery claim.
	ThreadSearch,
}
impl AppServerCapability {
	/// Complete closed capability set in stable report order.
	pub const ALL: [Self; 8] = [
		Self::Initialize,
		Self::AccountRead,
		Self::ThreadList,
		Self::ThreadRead,
		Self::ThreadArchive,
		Self::PaginatedHistory,
		Self::NativeCollaboration,
		Self::ThreadSearch,
	];
}

/// Readiness classification for one doctor component.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "state", content = "issue", rename_all = "snake_case")]
pub enum DoctorStatus {
	/// The server established readiness through the owning boundary.
	Ready,
	/// The component cannot currently serve its contract.
	Unavailable(DoctorIssue),
	/// No bounded observation currently establishes readiness or unavailability.
	Unknown(DoctorIssue),
}

/// Closed, redacted doctor failure classes and deterministic fixture outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorIssue {
	/// A required credential or authenticated boundary was unavailable.
	Authentication,
	/// Required plugin readiness was not established.
	Plugin,
	/// The typed operator configuration file was absent.
	ConfigurationMissing,
	/// The typed operator configuration was malformed.
	ConfigurationMalformed,
	/// The operator configuration schema version was unsupported.
	ConfigurationVersion,
	/// No explicit PostgreSQL configuration was available.
	DatabaseNotConfigured,
	/// PostgreSQL connection fields were malformed.
	DatabaseMalformedConfig,
	/// The explicit PostgreSQL endpoint could not be reached.
	DatabaseUnreachable,
	/// PostgreSQL exact-current verification found an incompatible state.
	DatabaseIncompatible,
	/// The steady-state PostgreSQL identity retains forbidden authority.
	UnsafeDatabaseAuthority,
	/// No connection to the daemon protocol was established.
	ProtocolDisconnected,
	/// The requested protocol revision was outside the compatibility window.
	ProtocolVersionMismatch,
	/// The connected server did not match the expected stable identity.
	ServerIdentityMismatch,
	/// No durable stable server identity could be loaded or created.
	ServerIdentityUnavailable,
	/// A server-host path failed its safety contract.
	UnsafeHostPath,
	/// Content or storage integrity could not be established.
	Integrity,
	/// The owning boundary was intentionally not probed.
	NotProbed,
	/// The capability is intentionally disabled by an active gate.
	Disabled,
}
impl DoctorIssue {
	/// Complete closed issue set in stable diagnostic order.
	pub const ALL: [Self; 18] = [
		Self::Authentication,
		Self::Plugin,
		Self::ConfigurationMissing,
		Self::ConfigurationMalformed,
		Self::ConfigurationVersion,
		Self::DatabaseNotConfigured,
		Self::DatabaseMalformedConfig,
		Self::DatabaseUnreachable,
		Self::DatabaseIncompatible,
		Self::UnsafeDatabaseAuthority,
		Self::ProtocolDisconnected,
		Self::ProtocolVersionMismatch,
		Self::ServerIdentityMismatch,
		Self::ServerIdentityUnavailable,
		Self::UnsafeHostPath,
		Self::Integrity,
		Self::NotProbed,
		Self::Disabled,
	];
}

/// A report violated its mechanical boundedness or uniqueness contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorContractError {
	/// The report exceeded [`MAX_DOCTOR_CHECKS`].
	TooManyChecks,
	/// More than one check named the same typed component.
	DuplicateComponent,
}
impl Display for DoctorContractError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::TooManyChecks => formatter.write_str("doctor report exceeds the check limit"),
			Self::DuplicateComponent => formatter.write_str("doctor report repeats a component"),
		}
	}
}

impl Error for DoctorContractError {}

#[cfg(test)]
mod tests {
	use crate::{
		CURRENT_VERSION, DoctorCheck, DoctorComponent, DoctorContractError, DoctorIssue,
		DoctorReport, DoctorStatus, MAX_DOCTOR_CHECKS, ServerId,
	};

	#[test]
	fn report_is_bounded_unique_and_redaction_by_construction() {
		let report = DoctorReport::new(
			ServerId::new("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162").unwrap(),
			CURRENT_VERSION,
			vec![
				DoctorCheck::new(DoctorComponent::ProductStore, DoctorStatus::Ready),
				DoctorCheck::new(
					DoctorComponent::ManagedRepository,
					DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath),
				),
			],
		)
		.unwrap();
		let encoded = serde_json::to_string(&report).unwrap();

		assert!(encoded.contains("unsafe_host_path"));
		assert!(!encoded.contains("/operator/private/repository"));
		assert!(!encoded.contains("credential-value"));
		assert_eq!(serde_json::from_str::<DoctorReport>(&encoded).unwrap(), report);

		let duplicate = DoctorReport::new(
			ServerId::new("server").unwrap(),
			CURRENT_VERSION,
			vec![
				DoctorCheck::new(DoctorComponent::Protocol, DoctorStatus::Ready),
				DoctorCheck::new(DoctorComponent::Protocol, DoctorStatus::Ready),
			],
		);

		assert_eq!(duplicate.unwrap_err(), DoctorContractError::DuplicateComponent);
	}

	#[test]
	fn oversized_report_is_rejected_on_construction_and_decode() {
		let checks = (0..=MAX_DOCTOR_CHECKS)
			.map(|_| DoctorCheck::new(DoctorComponent::Protocol, DoctorStatus::Ready))
			.collect();

		assert_eq!(
			DoctorReport::new(ServerId::new("server").unwrap(), CURRENT_VERSION, checks),
			Err(DoctorContractError::TooManyChecks)
		);

		let raw = serde_json::json!({
			"server_id": "server",
			"version": CURRENT_VERSION,
			"checks": (0..=MAX_DOCTOR_CHECKS).map(|_| serde_json::json!({
				"component": { "kind": "protocol" },
				"status": { "state": "ready" }
			})).collect::<Vec<_>>()
		});

		assert!(serde_json::from_value::<DoctorReport>(raw).is_err());
	}

	#[test]
	fn current_component_set_is_exact_and_order_independent() {
		let server = || ServerId::new("server").unwrap();
		let checks = || {
			DoctorComponent::ALL
				.into_iter()
				.map(|component| DoctorCheck::new(component, DoctorStatus::Ready))
				.collect::<Vec<_>>()
		};

		for incomplete in [Vec::new(), vec![checks()[0]], checks()[1..].to_vec()] {
			let report = DoctorReport::new(server(), CURRENT_VERSION, incomplete).unwrap();

			assert!(!report.has_current_component_set());
		}

		let mut arbitrary_order = checks();

		arbitrary_order.reverse();

		let report = DoctorReport::new(server(), CURRENT_VERSION, arbitrary_order).unwrap();

		assert!(report.has_current_component_set());
	}

	#[test]
	fn disconnected_version_server_auth_plugin_database_and_integrity_states_are_typed() {
		for issue in [
			DoctorIssue::ProtocolDisconnected,
			DoctorIssue::ProtocolVersionMismatch,
			DoctorIssue::ServerIdentityMismatch,
			DoctorIssue::Authentication,
			DoctorIssue::Plugin,
			DoctorIssue::DatabaseUnreachable,
			DoctorIssue::UnsafeDatabaseAuthority,
			DoctorIssue::Integrity,
		] {
			let encoded = serde_json::to_string(&DoctorStatus::Unavailable(issue)).unwrap();

			assert_eq!(
				serde_json::from_str::<DoctorStatus>(&encoded).unwrap(),
				DoctorStatus::Unavailable(issue)
			);
		}
	}
}
