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
	assert!(response.contains("\"SF Pro Display\", \"Avenir Next\""));
	assert!(response.contains("\"SFMono-Regular\", \"IBM Plex Mono\", \"Menlo\""));
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
