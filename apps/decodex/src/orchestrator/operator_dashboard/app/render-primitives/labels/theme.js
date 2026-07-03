			function renderThemeControls(selection, effectiveTheme) {
				for (const button of nodes.themeButtons) {
					const isActive = button.dataset.themeChoice === selection;
					button.classList.toggle("active", isActive);
					button.setAttribute("aria-pressed", isActive ? "true" : "false");
				}
			}

			function applyTheme(selection, persist = true) {
				themeSelection = ["system", "light", "dark"].includes(selection)
					? selection
					: "system";

				if (persist) {
					try {
						window.localStorage.setItem(THEME_STORAGE_KEY, themeSelection);
					} catch (_error) {
						/* Ignore storage failures and continue with the in-memory choice. */
					}
				}

				const effectiveTheme = resolveTheme(themeSelection);
				document.documentElement.dataset.theme = effectiveTheme;
				document.documentElement.style.colorScheme = effectiveTheme;
				renderThemeControls(themeSelection, effectiveTheme);
			}
