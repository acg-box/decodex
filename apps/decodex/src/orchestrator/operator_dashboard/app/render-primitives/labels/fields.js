			function renderField(label, value, valueClass, labelFormatter, fieldClass = "") {
				const fieldClassName = ["field", fieldClass].filter(Boolean).join(" ");
				const className = ["field-value", valueClass].filter(Boolean).join(" ");
				const valueHtml = renderValueLink(label, value) || escapeHtml(value);
				return `
					<div class="${fieldClassName}">
						<div class="field-label">${escapeHtml(labelFormatter(label))}</div>
						<div class="${className}">${valueHtml}</div>
					</div>
				`;
			}

			function field(label, value, valueClass = "") {
				return renderField(label, value, valueClass, detailLabel);
			}

			function cardField(label, value, valueClass = "") {
				return renderField(label, value, valueClass, titleCaseLabel, "card-field");
			}

			function cardFactValueClass(value, explicitClass = "") {
				return [explicitClass, String(value || "").trim() === "NONE" ? "is-muted" : ""]
					.filter(Boolean)
					.join(" ");
			}

			function optionalCardToken(value) {
				const token = String(value || "").trim();
				return token || "NONE";
			}

			function reviewThreadToken(count) {
				const numericCount = Number(count);
				return Number.isFinite(numericCount) && numericCount > 0 ? String(numericCount) : "NONE";
			}
