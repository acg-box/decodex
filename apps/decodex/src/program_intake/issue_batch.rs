pub(in crate::program_intake) mod identity;
pub(in crate::program_intake) mod nodes;
pub(in crate::program_intake) mod reporting;

mod config;

pub(crate) use self::config::{
	register_intake_project_config_for_persist, resolve_intake_project_config_path,
};
