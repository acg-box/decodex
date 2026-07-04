mod issue;
mod outcome;
mod refs;
mod run;
mod source;

pub(crate) use self::{
	issue::ResearchProposedIssueInput,
	outcome::ResearchDesignOutcome,
	refs::{ResearchPrivateEvidenceRefInput, ResearchPublicProjectionRefInput},
	run::ResearchDesignRunInput,
	source::{
		ResearchEvidenceInput, ResearchOptionInput, ResearchProvenanceInput, ResearchSubworkInput,
	},
};
