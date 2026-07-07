//! Autonomy signal classification enums.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalKind {
	RuntimeHealth,
	ValidationRegression,
	ReviewFeedbackCluster,
	UserFeedbackCluster,
	SpecDrift,
	ProtocolDrift,
	MetricRegression,
	ExecutionFriction,
	#[serde(alias = "docs_skill_drift")]
	DocsPluginDrift,
}
impl AutonomySignalKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::RuntimeHealth => "runtime_health",
			Self::ValidationRegression => "validation_regression",
			Self::ReviewFeedbackCluster => "review_feedback_cluster",
			Self::UserFeedbackCluster => "user_feedback_cluster",
			Self::SpecDrift => "spec_drift",
			Self::ProtocolDrift => "protocol_drift",
			Self::MetricRegression => "metric_regression",
			Self::ExecutionFriction => "execution_friction",
			Self::DocsPluginDrift => "docs_plugin_drift",
		}
	}

	pub(crate) fn matches_stored_kind(self, value: &str) -> bool {
		value == self.as_str()
			|| matches!(self, Self::DocsPluginDrift) && value == "docs_skill_drift"
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalSourceType {
	User,
	Review,
	Ci,
	Telemetry,
	Runtime,
	Docs,
	Protocol,
	Agent,
	Tracker,
	Memory,
	Report,
}
impl AutonomySignalSourceType {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::User => "user",
			Self::Review => "review",
			Self::Ci => "ci",
			Self::Telemetry => "telemetry",
			Self::Runtime => "runtime",
			Self::Docs => "docs",
			Self::Protocol => "protocol",
			Self::Agent => "agent",
			Self::Tracker => "tracker",
			Self::Memory => "memory",
			Self::Report => "report",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalFreshness {
	Fresh,
	Stale,
	Unknown,
}
impl AutonomySignalFreshness {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Fresh => "fresh",
			Self::Stale => "stale",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalEvidenceClass {
	ExternalSource,
	RepoSource,
	LiveReadback,
	Inference,
	Gap,
}
impl AutonomySignalEvidenceClass {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ExternalSource => "external_source",
			Self::RepoSource => "repo_source",
			Self::LiveReadback => "live_readback",
			Self::Inference => "inference",
			Self::Gap => "gap",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalConfidence {
	High,
	Medium,
	Low,
	Unknown,
}
impl AutonomySignalConfidence {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::High => "high",
			Self::Medium => "medium",
			Self::Low => "low",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomySignalPrivacy {
	Public,
	Team,
	LocalPrivate,
}
impl AutonomySignalPrivacy {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Public => "public",
			Self::Team => "team",
			Self::LocalPrivate => "local_private",
		}
	}
}
