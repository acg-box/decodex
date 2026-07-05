use std::collections::BTreeMap;

use crate::autonomy_signal::{
	AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
	AutonomySignalInput, AutonomySignalPrivacy, AutonomySignalSourceType,
};

pub(crate) fn signal_input() -> AutonomySignalInput {
	AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:runtime-health")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1086")),
		run_id: Some(String::from("xy-1086-attempt-1")),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("3cd19609c44cb18bff9e7a34a2f4853754afcee0")),
		captured_at: String::from("2026-06-22T00:00:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status readback showed repeated friction."),
		evidence: vec![String::from("status readback retained the repeated friction signal")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: vec![String::from("No dashboard comparison included.")],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-22T00:00:05Z"),
	}
}

pub(crate) fn runtime_signal() -> AutonomySignal {
	AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate")
}
