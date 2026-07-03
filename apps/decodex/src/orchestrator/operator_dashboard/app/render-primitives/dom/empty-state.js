			function renderEmptyState(title, copy = "") {
				const copyAttributes = copy
					? ` title="${escapeHtml(copy)}" aria-label="${escapeHtml(`${title}: ${copy}`)}"`
					: ` aria-label="${escapeHtml(title)}"`;
				return `
					<div class="empty-state"${copyAttributes}>
						<strong>${escapeHtml(title)}</strong>
					</div>
				`;
			}

			function renderRoutineEmptyList(container) {
				container.innerHTML = "";
			}
