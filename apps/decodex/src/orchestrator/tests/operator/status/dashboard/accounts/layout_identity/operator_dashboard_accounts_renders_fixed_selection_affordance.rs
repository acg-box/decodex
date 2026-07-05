use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_accounts_renders_fixed_selection_affordance() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("is-selected"));
	assert!(response.contains("is-ready"));
	assert!(response.contains("is-armed"));
	assert!(response.contains("--account-accent: var(--tone-muted);"));
	assert!(response.contains("--account-confirm-accent: var(--tone-run);"));
	assert!(
		response.contains(".account-row.is-ready {\n\t\t\t\t--account-accent: var(--success);")
	);
	assert!(response.contains(".account-row.is-fixed {\n\t\t\t\t--account-accent: var(--info);"));
	assert!(
		!response.contains(".account-row.is-armed {\n\t\t\t\t--account-accent: var(--warning);")
	);
	assert!(response.contains("--account-confirm-cycle: 1.45s;"));
	assert!(!response.contains("--account-confirm-color-cycle"));
	assert!(!response.contains("account-confirm-bar-breathe"));
	assert!(response.contains("@keyframes account-confirm-name-breathe"));
	assert!(response.contains("@keyframes account-confirm-bracket-left"));
	assert!(response.contains("@keyframes account-confirm-bracket-right"));
	assert!(response.contains("color: var(--account-confirm-accent);"));
	assert!(!response.contains("12.5%"));
	assert!(!response.contains("37.5%"));
	assert!(!response.contains("62.5%"));
	assert!(!response.contains("87.5%"));
	assert!(
		response.contains(
			"color: color-mix(in srgb, var(--account-confirm-accent) 46%, var(--muted));"
		)
	);
	assert!(response.contains("text-shadow: none;"));
	assert!(response.contains(".account-name-button.is-fixed::before"));
	assert!(response.contains(".account-name-button.is-fixed::after"));
	assert!(response.contains(
		".account-name-button.is-fixed {\n\t\t\t\tcolor: var(--account-confirm-accent);"
	));
	assert!(response.contains(".account-name-button + .account-name-reroll"));
	assert!(response.contains("margin-left: 8px;"));
	assert!(response.contains("opacity: 0.72;"));
	assert!(response.contains(
		"animation: account-confirm-name-breathe var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(response.contains(
		"animation: account-confirm-bracket-left var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(response.contains(
		"animation: account-confirm-bracket-right var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(!response.contains("infinite alternate;"));
}
