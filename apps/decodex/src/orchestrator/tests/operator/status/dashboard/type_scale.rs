use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_uses_shared_type_scale_for_operator_rows() {
	let response = dashboard::dashboard_response();
	let section_marker_title = response
		.split(".section-marker > span {")
		.nth(1)
		.expect("section marker title style should exist")
		.split(".section-marker > span::before")
		.next()
		.expect("section marker title style should end before marker rule");
	let panel_title = response
		.split(".panel-head h2 {")
		.nth(1)
		.expect("panel title style should exist")
		.split(".panel:hover .panel-head h2")
		.next()
		.expect("panel title style should end before hover rule");
	let section_marker = response
		.split(".section-marker {")
		.nth(1)
		.expect("section marker style should exist")
		.split(".section-marker:first-child")
		.next()
		.expect("section marker style should end before first child rule");
	let section_marker_bar = response
		.split(".section-marker > span::before {")
		.nth(1)
		.expect("section marker bar style should exist")
		.split(".section-marker-control")
		.next()
		.expect("section marker bar style should end before meta rule");
	let flow_step_label = response
		.split(".flow-step span {")
		.nth(1)
		.expect("flow step label style should exist")
		.split(".flow-step-labels")
		.next()
		.expect("flow step label style should end before grid rule");
	let table_meta = response
		.split(".table-meta {")
		.nth(1)
		.expect("table meta style should exist")
		.split(".table-meta:empty")
		.next()
		.expect("table meta style should end before empty rule");
	let metric_number = response
		.split(".metric-number {")
		.nth(1)
		.expect("metric number style should exist")
		.split(".metric-label")
		.next()
		.expect("metric number style should end before metric label rule");

	assert!(response.contains("--type-micro: 10px;"));
	assert!(response.contains("--type-caption: 11px;"));
	assert!(response.contains("--type-label: 12px;"));
	assert!(response.contains("--type-body: 13px;"));
	assert!(response.contains("--type-row-title: 13px;"));
	assert!(response.contains("--type-section-title: 13px;"));
	assert!(response.contains("--weight-label: 500;"));
	assert!(response.contains("--weight-strong: 600;"));
	assert!(response.contains("--tracking-caps: 0;"));
	assert!(response.contains("--tone-ready: #1d8968;"));
	assert!(response.contains("--tone-ready: #5cc59f;"));
	assert!(response.contains(".tone-ready"));
	assert!(response.contains("-apple-system, BlinkMacSystemFont"));
	assert!(response.contains("\"Menlo\", \"Monaco\", \"SFMono-Regular\", \"SF Mono\""));
	assert!(response.contains("ui-monospace, \"Cascadia Mono\","));
	assert!(response.contains("--space-panel-head-y: 12px;"));
	assert!(response.contains("--space-row-y: 12px;"));
	assert!(response.contains("--space-card-y: 16px;"));
	assert!(response.contains("--space-row-indent: 18px;"));
	assert!(section_marker_title.contains("font-size: var(--type-section-title);"));
	assert!(section_marker_title.contains("font-weight: var(--weight-label);"));
	assert!(section_marker.contains("font-family: var(--sans);"));
	assert!(!section_marker_title.contains("text-transform: uppercase;"));
	assert!(section_marker_bar.contains("height: 14px;"));
	assert!(!response.contains(".section-marker > .table-meta {"));
	assert!(flow_step_label.contains("font-family: var(--mono);"));
	assert!(flow_step_label.contains("font-size: var(--type-label);"));
	assert!(flow_step_label.contains("font-weight: var(--weight-label);"));
	assert!(!flow_step_label.contains("text-transform: uppercase;"));
	assert!(flow_step_label.contains("color: var(--muted-strong);"));
	assert!(panel_title.contains("font-size: var(--type-section-title);"));
	assert!(panel_title.contains("font-weight: var(--weight-label);"));
	assert!(panel_title.contains("font-family: var(--sans);"));
	assert!(!panel_title.contains("text-transform: uppercase;"));
	assert!(table_meta.contains("font-family: var(--mono);"));
	assert!(table_meta.contains("font-weight: var(--weight-label);"));
	assert!(!table_meta.contains("text-transform: uppercase;"));
	assert!(metric_number.contains("font-size: 0.92em;"));
	assert!(metric_number.contains("font-weight: var(--weight-label);"));
	assert!(response.contains("padding: var(--space-panel-head-y) 0 var(--space-md);"));
	assert!(
		response
			.contains("padding: var(--space-row-y) 0 var(--space-row-y) var(--space-row-indent);")
	);
	assert!(
		response.contains(
			"padding: var(--space-card-y) 0 var(--space-card-y) var(--space-row-indent);"
		)
	);
	assert!(response.contains(".project-title-line strong"));
	assert!(response.contains(".transport-meta[data-kind=\"endpoint\"] strong"));
	assert!(response.contains(".run-meta-item.is-missing strong"));
	assert!(response.contains(".project-work-ratio strong"));
	assert!(response.contains(".metric-number"));
	assert!(response.contains(".metric-label"));
	assert!(response.contains(".metric-group"));
	assert!(response.contains("gap: 4px;"));
	assert!(response.contains("font-variant-numeric: tabular-nums;"));
	assert!(response.contains(".account-row-id strong"));
	assert!(response.contains("font-size: var(--type-body);"));
	assert!(!response.contains("font-size: 17px;"));
	assert!(!response.contains("letter-spacing: 0.14em;"));
	assert!(!response.contains("font-weight: 700;"));
	assert!(!response.contains("--weight-label: 650;"));
	assert!(!response.contains("--weight-strong: 650;"));
	assert!(!response.contains("padding: 18px 0 18px 18px;"));
	assert!(!response.contains("padding: 20px 0 12px;"));
}

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
