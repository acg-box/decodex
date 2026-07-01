
			function escapeHtml(value) {
				return String(value).replace(/[&<>"']/g, (character) => {
					switch (character) {
						case "&":
							return "&amp;";
						case "<":
							return "&lt;";
						case ">":
							return "&gt;";
						case "\"":
							return "&quot;";
						case "'":
							return "&#39;";
						default:
							return character;
					}
				});
			}

			function encodeLocalPath(path) {
				return path
					.split("/")
					.map((segment, index) => (index === 0 ? "" : encodeURIComponent(segment)))
					.join("/");
			}

			function localPathHref(value) {
				const rawValue = String(value || "").trim();
				if (
					!rawValue.startsWith("/") ||
					rawValue.startsWith("//") ||
					/[\n\r]/.test(rawValue) ||
					!["/Users/", "/Volumes/", "/tmp/", "/private/", "/var/", "/opt/", "/home/"].some((prefix) =>
						rawValue.startsWith(prefix),
					)
				) {
					return "";
				}

				return `file://${encodeLocalPath(rawValue)}`;
			}

			function linkHref(value) {
				const rawValue = String(value || "").trim();
				return /^(https?|wss?):\/\//i.test(rawValue) ? rawValue : localPathHref(rawValue);
			}

			function linkValueLabel(label, value) {
				const text = String(value || "").trim();
				const labelKey = detailLabel(label).toLowerCase();
				const pullRequestMatch = text.match(/\/pull\/(\d+)(?:$|[/?#])/);
				if (labelKey === "pr" && pullRequestMatch) {
					return `#${pullRequestMatch[1]}`;
				}

				return text;
			}

			function renderValueLink(label, value, className = "value-link") {
				const href = linkHref(value);
				if (!href) {
					return "";
				}

				return `<a class="${className}" href="${escapeHtml(href)}" target="_blank" rel="noreferrer" title="${escapeHtml(href)}">${escapeHtml(linkValueLabel(label, value))}</a>`;
			}

			function metricTokenParts(token) {
				const match = String(token).trim().match(/^(-?\d[\d.,]*(?:[a-zA-Z%]+)?)(?:\s+(.+))?$/);
				if (!match) {
					return { label: token };
				}

				return {
					number: match[1],
					label: match[2] || "",
				};
			}

			function renderMetricGroup(token) {
				const parts = metricTokenParts(token);
				if (parts.number == null) {
					return `<span class="metric-group"><span class="metric-label">${escapeHtml(parts.label)}</span></span>`;
				}

				const label = parts.label
					? `<span class="metric-label">${escapeHtml(titleCaseLabel(parts.label))}</span>`
					: "";
				return `<span class="metric-group"><span class="metric-number">${escapeHtml(parts.number)}</span>${label}</span>`;
			}

			function renderMetricText(text) {
				const tokens = String(text)
					.split(" · ")
					.map((token) => token.trim())
					.filter(Boolean);

				if (!tokens.length) {
					return "";
				}

				return `<span class="metric-text">${tokens
					.map((token, index) => {
						const separator =
							index === 0 ? "" : '<span class="metric-separator"> · </span>';
						return `${separator}${renderMetricGroup(token)}`;
					})
					.join("")}</span>`;
			}

			function setMetricText(node, text) {
				node.innerHTML = renderMetricText(text);
			}

			function displayToken(value) {
				const token = String(value ?? "").trim();
				return token || "none";
			}

			function normalizedDisplayText(value) {
				return String(value || "")
					.toLowerCase()
					.replace(/[^a-z0-9]+/g, " ")
					.trim();
			}

			function displayTextRepeats(left, right) {
				const normalizedLeft = normalizedDisplayText(left);
				const normalizedRight = normalizedDisplayText(right);

				return Boolean(
					normalizedLeft &&
						normalizedRight &&
						(normalizedLeft === normalizedRight ||
							normalizedLeft.includes(normalizedRight) ||
							normalizedRight.includes(normalizedLeft)),
				);
			}

			function compactStateToken(value) {
				return formatDetailToken(value);
			}
