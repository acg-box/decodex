use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeEvidence, AutonomyProposalChallengeInput,
	},
	prelude::Result,
};

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn record_challenge(&mut self, input: AutonomyProposalChallengeInput) -> Result<()> {
		let challenge = AutonomyProposalChallengeEvidence::from_input(input)?;
		let mut candidate = self.clone();

		candidate.challenge_evidence.push(challenge);
		candidate.validate()?;

		*self = candidate;

		Ok(())
	}
}
