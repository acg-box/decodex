use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_accounts_keeps_identity_rows_compact() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("grid-template-areas:"));
	assert!(response.contains("\"id plan primary secondary credit state\""));
	assert!(response.contains("\"meta meta meta meta meta meta\""));
	assert!(!response.contains("\"account status\""));
	assert!(!response.contains("\"windows windows\""));
	assert!(response.contains(".account-row-id {\n\t\t\t\tgrid-area: id;"));
	assert!(response.contains("justify-content: center;"));
	assert!(response.contains("text-align: center;"));
	assert!(response.contains("function codexAccountCapacityLabel(account)"));
	assert!(response.contains("function codexAccountCapacityMultiplier(account)"));
	assert!(
		response
			.contains("const planType = String(account?.plan_type || \"\").trim().toLowerCase();")
	);
	assert!(response.contains("return planType === \"pro\" ? 20 : 1;"));
	assert!(response.contains("const weight = codexAccountCapacityLabel(account);"));
	assert!(response.contains(
		"const identityClass = codexAccountShowsEmail(account) ? \" is-machine\" : \"\";"
	));
	assert!(response.contains(".account-row-plan {\n\t\t\t\tgrid-area: plan;"));
	assert!(response.contains("<div class=\"account-row-id${identityClass}\">"));
	assert!(response.contains("<div class=\"account-row-plan\">${escapeHtml(weight)}</div>"));
	assert!(response.contains("<button class=\"account-name-button${fixedClass}${armedClass}\""));
	assert!(response.contains("<span class=\"account-name\">${escapeHtml(visibleName)}</span>"));
	assert!(response.contains("<span class=\"run-meta-icon\" aria-hidden=\"true\">"));
	assert!(response.contains("<svg viewBox=\"0 0 16 16\" fill=\"none\">"));
	assert!(response.contains(
		"<path fill=\"currentColor\" fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.35 2.25h9.3"
	));
	assert!(response.contains("M8 4.15a1.8 1.8"));
	assert!(response.contains("c.61 0 1.1.49 1.1 1.1v9.3"));
	assert!(!response.contains("d=\"M8 1.65a6.35"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M8 7.3a2.55"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M3.25 13.15c.48-2.65"));
	assert!(!response.contains("fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.9 2.2h8.2"));
	assert!(!response.contains("<circle cx=\"8\" cy=\"5.1\""));
	assert!(response.contains("<strong class=\"account-name${identityClass}\" title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>"));
	assert!(
		!response.contains("<strong class=\"machine-text\">${escapeHtml(`${value}%`)}</strong>")
	);
	assert!(!response.contains("function codexAccountSecondaryLabel(account)"));
	assert!(response.contains("const visibleName = codexAccountVisibleName(account);"));
	assert!(response.contains("const displayTitle = codexAccountDisplayTitle(account);"));
	assert!(
		response
			.contains("title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>")
	);
	assert!(response.contains("text.startsWith(\"...\") && text.indexOf(\"...\", 3) === -1"));
}
