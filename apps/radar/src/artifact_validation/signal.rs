//! Signal and config-feature artifact validation.

pub(crate) mod text;

mod catalog;
mod references;
mod root;

pub(super) use self::{catalog::validate_config_feature_catalog, root::validate_signal};
