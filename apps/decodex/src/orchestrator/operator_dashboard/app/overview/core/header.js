			function renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError) {
				nodes.projectTitle.textContent = "Decodex";
				document.title = snapshot
					? `${snapshot.project_id} · Decodex`
					: "Decodex";
				const snapshotFreshness = snapshotFreshnessMeta(
					snapshotPublishedAt,
					readiness,
					snapshotError,
				);
				const snapshotFreshnessRow = snapshotFreshness
					? `
						<span class="transport-meta" data-kind="snapshot" data-tone="${escapeHtml(snapshotFreshness.tone)}" title="${escapeHtml(snapshotFreshness.title)}">
							<span>Snapshot</span><strong>${escapeHtml(snapshotFreshness.label)}</strong>
						</span>
					`
					: "";
				const stream = dashboardStreamMeta();

				nodes.transportHealth.innerHTML = `
					<span class="status-pill ${readiness.tone}">${escapeHtml(topbarReadinessLabel(readiness.label))}</span>
					<span class="transport-meta" data-kind="endpoint" data-tone="${escapeHtml(stream.tone)}" title="${escapeHtml(stream.title)}">
						<span>Transport</span><strong>${renderValueLink("WebSocket", dashboardSocketUrl(), "transport-link") || escapeHtml(dashboardSocketUrl())}</strong>
					</span>
					${snapshotFreshnessRow}
				`;
				renderNoticeDock(notices);
			}
