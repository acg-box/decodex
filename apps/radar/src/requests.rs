//! Public Radar command request and report contracts.

mod bundle;
mod cache;
mod content;
mod ledger;
mod release_delta;
mod signal;
mod validation;

#[cfg(test)] pub(crate) use self::cache::CacheRetentionPolicy;
pub(crate) use self::{
	bundle::{RadarBundleBuildReceipt, RadarBundleBuildRequest, RadarBundleValidateRequest},
	cache::{
		RadarCacheGcReport, RadarCacheGcRequest, RadarContentV2ResetReport,
		RadarContentV2ResetRequest,
	},
	content::{
		RadarContentEligibilityReport, RadarContentEligibilityRequest,
		RadarContentPairCommitReport, RadarContentPairCommitRequest, RadarQueueGeneration,
		RadarReviewNextReport, RadarReviewNextRequest, RadarSelectedSubject, RadarSourceRef,
	},
	ledger::{
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	},
	release_delta::{
		RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest,
		RadarRefreshReleaseDeltaReport, RadarRefreshReleaseDeltaRequest,
	},
	signal::{RadarRenderSignalReport, RadarRenderSignalRequest},
	validation::{
		RadarRefreshQueueReport, RadarRefreshQueueRequest, RadarValidateRequest,
		RadarValidationReport,
	},
};
