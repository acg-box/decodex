use std::error::Error;

pub(super) fn decision_contract_load_error_is_quarantinable(error: &(dyn Error + 'static)) -> bool {
	let message = error.to_string();

	message.contains("Decision Contract proposed issue `")
		&& (message.contains("depends on unknown issue")
			|| message.contains("must not depend on itself")
			|| message.contains("dependency cycle includes"))
}

pub(super) fn autonomy_proposal_load_error_is_quarantinable(error: &(dyn Error + 'static)) -> bool {
	let message = error.to_string();

	message.contains("Autonomy proposal `") && message.contains("fingerprint mismatch")
}
