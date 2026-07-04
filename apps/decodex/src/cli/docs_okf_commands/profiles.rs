use clap::ValueEnum;

use crate::docs_okf::OkfCheckProfile;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum OkfProfileArg {
	Core,
	Wiki,
	RepoMemory,
	Decodex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum OkfInitProfileArg {
	Core,
	Wiki,
	RepoMemory,
}

impl From<OkfProfileArg> for OkfCheckProfile {
	fn from(value: OkfProfileArg) -> Self {
		match value {
			OkfProfileArg::Core => Self::Core,
			OkfProfileArg::Wiki => Self::Wiki,
			OkfProfileArg::RepoMemory => Self::RepoMemory,
			OkfProfileArg::Decodex => Self::Decodex,
		}
	}
}

impl From<OkfInitProfileArg> for OkfCheckProfile {
	fn from(value: OkfInitProfileArg) -> Self {
		match value {
			OkfInitProfileArg::Core => Self::Core,
			OkfInitProfileArg::Wiki => Self::Wiki,
			OkfInitProfileArg::RepoMemory => Self::RepoMemory,
		}
	}
}
