			function renderNoticeDock(notices) {
				const hasNotices = notices.length > 0;
				nodes.noticeDock.classList.toggle("visible", hasNotices);
				nodes.noticeDock.setAttribute("aria-hidden", hasNotices ? "false" : "true");

				if (!hasNotices) {
					nodes.noticeDock.removeAttribute("open");
					delete nodes.noticeDock.dataset.tone;
					nodes.noticeCount.textContent = "0";
					nodes.noticeLabel.textContent = "notices";
					nodes.noticeList.innerHTML = "";

					return;
				}

				const tone = notices.some((notice) => notice.tone === "danger") ? "danger" : "warning";
				const dangerCount = notices.filter((notice) => notice.tone === "danger").length;
				nodes.noticeDock.dataset.tone = tone;
				nodes.noticeCount.textContent = String(notices.length);
				nodes.noticeLabel.textContent =
					dangerCount > 0
						? pluralLabel(notices.length, "alert")
						: pluralLabel(notices.length, "warning");
				nodes.noticeList.innerHTML = notices
					.map(
						(notice) => `
							<article class="notice-item ${notice.tone}">
								<strong>${escapeHtml(notice.title)}</strong>
								<p>${escapeHtml(notice.copy)}</p>
								${notice.ackKey ? `<button class="control-button" type="button" data-notice-ack="${escapeHtml(notice.ackKey)}">Ack</button>` : ""}
							</article>
						`,
					)
					.join("");
			}
