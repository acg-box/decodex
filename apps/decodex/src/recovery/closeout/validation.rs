mod legacy;
mod merged;

pub(super) use self::{
	legacy::validate_legacy_closeout_request, merged::validate_merged_closeout_request,
};
