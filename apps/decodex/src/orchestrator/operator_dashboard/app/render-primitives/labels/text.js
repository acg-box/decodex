
			function titleCaseLabel(label) {
				return String(label || "").replace(/\b[A-Za-z][A-Za-z0-9]*\b/g, (word) => {
					if (/^[A-Z0-9]+$/.test(word) && /[A-Z]/.test(word)) {
						return word;
					}
					const lower = word.toLowerCase();
					const acronym = FIELD_LABEL_ACRONYMS.get(lower);
					return acronym || `${lower.charAt(0).toUpperCase()}${lower.slice(1)}`;
				});
			}

			function detailLabel(label) {
				return String(label || "").replace(/\b[A-Za-z][A-Za-z0-9]*\b/g, (word) => {
					const lower = word.toLowerCase();
					return FIELD_LABEL_ACRONYMS.get(lower) || lower;
				});
			}


			function resolveTheme(selection) {
				if (selection === "dark" || selection === "light") {
					return selection;
				}

				return themeMediaQuery.matches ? "dark" : "light";
			}
