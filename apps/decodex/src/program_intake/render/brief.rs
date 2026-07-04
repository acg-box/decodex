use crate::{
	loop_contract::DecisionContract,
	prelude::Result,
	program_intake::{goal, model::GoalIssueBriefInput, render::validation},
};

pub(crate) fn render_goal_issue_brief(input: GoalIssueBriefInput<'_>) -> Result<String> {
	let mut output = String::new();

	append_heading(&mut output, "Objective");

	output.push_str(input.objective.trim());
	output.push('\n');

	append_heading(&mut output, "Authority");
	append_item(
		&mut output,
		"Accepted Decision Contract authority is recorded in Decodex runtime state.",
	);
	append_optional_item(
		&mut output,
		"Source issue",
		input.contract.source_intent().source_issue_identifier(),
	);
	append_heading(&mut output, "Required Reading");
	append_item(&mut output, "This issue brief.");
	append_optional_item(
		&mut output,
		"Linked source issue",
		input.contract.source_intent().source_issue_identifier(),
	);
	append_heading(&mut output, "Scope");
	append_item(&mut output, input.objective.trim());
	append_items(&mut output, input.contract.accepted_authority().accepted_objectives());
	append_items(&mut output, input.contract.accepted_authority().constraints());
	append_items(&mut output, input.contract.accepted_authority().assumptions());
	append_heading(&mut output, "Ownership Boundary");
	append_item(
		&mut output,
		"Work only on this generated issue's objective and avoid unrelated cleanup.",
	);
	append_item(
		&mut output,
		"Do not use Program graph ids, queue labels, or private runtime state as execution authority.",
	);
	append_heading(&mut output, "Non-goals");
	append_items_or_none(&mut output, input.contract.accepted_authority().non_goals());
	append_heading(&mut output, "Dependencies");
	append_items_or_none(&mut output, input.dependencies);
	append_heading(&mut output, "Conflict Domains");
	append_items_or_none(&mut output, &goal::conflict_domain_labels(input.conflict_domains));
	append_heading(&mut output, "Current-tree Landing Zone");
	append_items_or_none(&mut output, &goal::conflict_domain_labels(input.conflict_domains));
	append_heading(&mut output, "Acceptance");
	append_items(&mut output, input.acceptance);
	append_heading(&mut output, "Validation");
	append_items(&mut output, input.validation);
	append_heading(&mut output, "Lifecycle Gates");
	append_item(&mut output, "Run the repo-native validation gate before review handoff.");
	append_item(
		&mut output,
		"Use normal Decodex review, PR handoff, landing, closeout, and cleanup gates.",
	);
	append_item(
		&mut output,
		"Run install or restart steps only when the owning issue or workflow requires them.",
	);
	append_heading(&mut output, "Risk");
	append_items_or_none(&mut output, input.risk);
	append_heading(&mut output, "Stop Conditions");
	append_items_or_none(&mut output, input.contract.accepted_authority().stop_conditions());

	validation::validate_public_issue_description(&output)?;

	Ok(output)
}

pub(crate) fn generated_issue_private_identifiers(
	contract: &DecisionContract,
	program_id: &str,
	node_id: &str,
) -> Vec<String> {
	let mut identifiers = vec![program_id.to_owned(), node_id.to_owned()];

	identifiers.extend(contract.research_provenance().iter().filter_map(|provenance| {
		if matches!(provenance.kind(), "autonomy_objective" | "autonomy_proposal") {
			Some(provenance.reference().to_owned())
		} else {
			None
		}
	}));
	identifiers.extend(contract.research_evidence().iter().filter_map(|evidence| {
		if evidence.kind().starts_with("autonomy_signal:") {
			evidence.source_ref().map(str::to_owned)
		} else {
			None
		}
	}));
	identifiers.sort();
	identifiers.dedup();

	identifiers
}

fn append_heading(output: &mut String, heading: &str) {
	if !output.is_empty() {
		output.push('\n');
	}

	output.push_str("## ");
	output.push_str(heading);
	output.push('\n');
}

fn append_item(output: &mut String, item: &str) {
	output.push_str("- ");
	output.push_str(item);
	output.push('\n');
}

fn append_optional_item(output: &mut String, label: &str, value: Option<&str>) {
	if let Some(value) = value {
		append_item(output, &format!("{label}: `{value}`"));
	}
}

fn append_items(output: &mut String, items: &[String]) {
	for item in items {
		append_item(output, item);
	}
}

fn append_items_or_none(output: &mut String, items: &[String]) {
	if items.is_empty() {
		append_item(output, "None declared by the accepted Decision Contract.");
	} else {
		append_items(output, items);
	}
}
