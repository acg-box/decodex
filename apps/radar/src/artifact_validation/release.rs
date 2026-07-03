//! Release delta artifact validation.

mod compare;
mod options;
mod root;

pub(super) use self::root::validate_release_delta;
