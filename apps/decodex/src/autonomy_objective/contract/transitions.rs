use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveContract, AutonomyObjectiveRejection,
		AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	prelude::{Result, eyre},
};

#[allow(dead_code)]
impl AutonomyObjectiveContract {
	pub(crate) fn accept(&mut self, acceptance: AutonomyObjectiveAcceptance) -> Result<()> {
		match self.state {
			AutonomyObjectiveState::Draft => {},
			AutonomyObjectiveState::Accepted => {
				eyre::bail!(
					"Autonomy objective `{}` version {} is already accepted.",
					self.id,
					self.version
				);
			},
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded => {
				eyre::bail!(
					"Autonomy objective `{}` version {} cannot be accepted from state `{}`.",
					self.id,
					self.version,
					self.state.as_str()
				);
			},
		}

		acceptance.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Accepted;
		candidate.acceptance = Some(acceptance);
		candidate.rejection = None;
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn reject(&mut self, rejection: AutonomyObjectiveRejection) -> Result<()> {
		if self.state != AutonomyObjectiveState::Draft {
			eyre::bail!(
				"Autonomy objective `{}` version {} can only be rejected from draft state.",
				self.id,
				self.version
			);
		}

		rejection.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Rejected;
		candidate.acceptance = None;
		candidate.rejection = Some(rejection);
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn supersede(&mut self, supersession: AutonomyObjectiveSupersession) -> Result<()> {
		if matches!(
			self.state,
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded
		) {
			eyre::bail!(
				"Autonomy objective `{}` version {} cannot be superseded from state `{}`.",
				self.id,
				self.version,
				self.state.as_str()
			);
		}

		supersession.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Superseded;
		candidate.rejection = None;
		candidate.supersession = Some(supersession);

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}
}
