use crate::agent::app_server::preflight::{
	BTreeMap, Serialize,
	report::{
		check::{self, AppServerCapabilityPreflightCheck},
		status::AppServerCapabilityPreflightStatus,
	},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AppServerCapabilityPreflightReport {
	checks: Vec<AppServerCapabilityPreflightCheck>,
}
impl AppServerCapabilityPreflightReport {
	pub(crate) fn new() -> Self {
		Self { checks: Vec::new() }
	}

	#[cfg(test)]
	pub(crate) fn checks(&self) -> &[AppServerCapabilityPreflightCheck] {
		&self.checks
	}

	pub(crate) fn check_count(&self) -> usize {
		self.checks.len()
	}

	pub(crate) fn push_ok(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Ok,
			summary: summary.into(),
			details,
		});
	}

	pub(crate) fn push_blocked(
		&mut self,
		name: &'static str,
		summary: impl Into<String>,
		details: BTreeMap<String, String>,
	) {
		self.checks.push(AppServerCapabilityPreflightCheck {
			name,
			status: AppServerCapabilityPreflightStatus::Blocked,
			summary: summary.into(),
			details,
		});
	}

	pub(crate) fn has_blockers(&self) -> bool {
		self.checks.iter().any(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
	}

	pub(crate) fn blocker_summary(&self) -> String {
		let blockers = self
			.checks
			.iter()
			.filter(|check| check.status == AppServerCapabilityPreflightStatus::Blocked)
			.map(check::blocker_summary)
			.collect::<Vec<_>>();

		if blockers.is_empty() { String::from("no blockers recorded") } else { blockers.join("; ") }
	}
}
