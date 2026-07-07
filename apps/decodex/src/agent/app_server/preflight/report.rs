mod check;
mod failure;
mod model;
mod status;

#[cfg(test)]
pub(crate) use self::status::AppServerCapabilityPreflightStatus;
pub(crate) use self::{
	check::check_name_for_method, failure::AppServerCapabilityPreflightFailure,
	model::AppServerCapabilityPreflightReport,
};
