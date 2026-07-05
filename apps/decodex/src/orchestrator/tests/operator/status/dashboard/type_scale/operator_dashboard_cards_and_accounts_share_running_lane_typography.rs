use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_cards_and_accounts_share_running_lane_typography() {
	let response = dashboard::dashboard_response();
	let row_title = response
		.split(".row-title h3,\n\t\t\t.row-title h4,\n\t\t\t.run-title {")
		.nth(1)
		.expect("shared row title style should exist")
		.split(".worktree-card .row-summary")
		.next()
		.expect("shared row title style should end before worktree summary rule");
	let run_meta_line = response
		.split(".run-meta-line {")
		.nth(1)
		.expect("run meta line style should exist")
		.split(".run-meta-item {")
		.next()
		.expect("run meta line style should end before item rule");
	let account_row_id = response
		.split(".account-row-id {")
		.nth(1)
		.expect("account row id style should exist")
		.split(".account-row-id strong")
		.next()
		.expect("account row id style should end before strong rule");
	let row_aside = response
		.split(".row-aside {")
		.nth(1)
		.expect("row aside style should exist")
		.split(".run-title-stack")
		.next()
		.expect("row aside style should end before run title stack rule");
	let run_meta_icon = response
		.split(".run-meta-icon {")
		.nth(1)
		.expect("run meta icon style should exist")
		.split(".run-meta-icon svg")
		.next()
		.expect("run meta icon style should end before svg rule");

	assert!(response.contains(".run-title"));
	assert!(response.contains("[\"codex\", \"Codex\"]"));
	assert!(response.contains("[\"prs\", \"PRs\"]"));
	assert!(response.contains("titleCaseLabel(parts.label)"));
	assert!(response.contains("function detailLabel(label)"));
	assert!(
		response.contains("function renderValueLink(label, value, className = \"value-link\")")
	);
	assert!(response.contains("function localPathHref(value)"));
	assert!(response.contains("return `file://${encodeLocalPath(rawValue)}`;"));
	assert!(
		response.contains("const pullRequestMatch = text.match(/\\/pull\\/(\\d+)(?:$|[/?#])/);")
	);
	assert!(response.contains("return `#${pullRequestMatch[1]}`;"));
	assert!(response.contains("text-decoration: none;"));
	assert!(response.contains("background: color-mix(in srgb, currentColor 10%, transparent);"));
	assert!(
		response
			.contains("box-shadow: 0 0 0 1px color-mix(in srgb, currentColor 18%, transparent);")
	);
	assert!(response.contains(
		"function renderField(label, value, valueClass, labelFormatter, fieldClass = \"\")"
	));
	assert!(
		response.contains(
			"const fieldClassName = [\"field\", fieldClass].filter(Boolean).join(\" \");"
		)
	);
	assert!(response.contains("<div class=\"${fieldClassName}\">"));
	assert!(
		response.contains("<div class=\"field-label\">${escapeHtml(labelFormatter(label))}</div>")
	);
	assert!(response.contains("return renderField(label, value, valueClass, detailLabel);"));
	assert!(
		response.contains(
			"return renderField(label, value, valueClass, titleCaseLabel, \"card-field\");"
		)
	);
	assert!(
		response.contains("const valueHtml = renderValueLink(label, value) || escapeHtml(value);")
	);
	assert!(
		!response.contains("<div class=\"field-label\">${escapeHtml(titleCaseLabel(label))}</div>")
	);
	assert!(row_title.contains("font-size: var(--type-row-title);"));
	assert!(row_title.contains("font-weight: var(--weight-strong);"));
	assert!(row_title.contains("line-height: 1.28;"));
	assert!(response.contains("--type-row-aside: 11px;"));
	assert!(row_aside.contains("font-size: var(--type-row-aside);"));
	assert!(row_aside.contains("font-family: var(--mono);"));
	assert!(row_aside.contains("font-variant-numeric: tabular-nums;"));
	assert!(row_aside.contains("line-height: 1.22;"));
	assert!(response.contains("class=\"row-aside\""));
	assert!(!response.contains("class=\"card-meta\""));
	assert!(!response.contains("font-size: var(--type-card-title);"));
	assert!(!response.contains("--type-card-title:"));
	assert!(run_meta_line.contains("font-family: var(--mono);"));
	assert!(run_meta_line.contains("font-variant-ligatures: none;"));
	assert!(run_meta_line.contains("letter-spacing: 0;"));
	assert!(run_meta_line.contains("line-height: 1.35;"));
	assert!(response.contains(".run-meta-item.is-account {\n\t\t\t\tposition: relative;"));
	assert!(response.contains("padding-left: 16px;"));
	assert!(!response.contains(".run-meta-item.is-account {\n\t\t\t\talign-items: center;"));
	assert!(response.contains(".run-meta-icon {"));
	assert!(response.contains(".run-meta-icon svg"));
	assert!(run_meta_icon.contains("position: absolute;"));
	assert!(run_meta_icon.contains("top: 50%;"));
	assert!(run_meta_icon.contains("width: 12px;"));
	assert!(run_meta_icon.contains("height: 12px;"));
	assert!(run_meta_icon.contains("transform: translateY(-50%);"));
	assert!(response.contains(".run-meta-label"));
	assert!(account_row_id.contains("font-family: var(--mono);"));
	assert!(account_row_id.contains("font-variant-ligatures: none;"));
	assert!(account_row_id.contains("letter-spacing: 0;"));
	assert!(!response.contains(".account-row-id.is-machine"));
	assert!(!response.contains(".account-use-line"));
	assert!(!response.contains(".account-use-label"));
}
