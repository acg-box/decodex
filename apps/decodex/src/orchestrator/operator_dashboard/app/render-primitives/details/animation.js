			function setFoldPanelEmpty(panel, isEmpty) {
				panel.classList.toggle("is-empty", isEmpty);
				if (!isEmpty) {
					return;
				}

				const detailKey = detailStateKey(panel);
				if (detailKey) {
					detailDisclosureState.delete(detailKey);
				}
				const existingTimer = detailAnimationTimers.get(panel);
				if (existingTimer) {
					window.clearTimeout(existingTimer);
					detailAnimationTimers.delete(panel);
				}
				const content = detailContent(panel);
				if (content) {
					clearDetailAnimation(panel, content);
				}
				panel.open = true;
				setDetailVisualState(panel, false);
			}

			function clearDetailAnimation(details, content) {
				details.classList.remove("is-animating");
				content.style.height = "";
				content.style.opacity = "";
				content.style.overflow = "";
				content.style.transform = "";
			}

			function scrollOpenedDetailIntoView(details, content) {
				if (reducedMotionQuery.matches) {
					return;
				}

				window.requestAnimationFrame(() => {
					const viewportHeight =
						window.innerHeight || document.documentElement.clientHeight;
					const contentRect = content.getBoundingClientRect();
					const targetBottom = Math.min(
						contentRect.bottom,
						contentRect.top + Math.min(content.scrollHeight, viewportHeight * 0.62),
					);
					const overflow = targetBottom - (viewportHeight - 28);

					if (overflow > 0) {
						window.scrollBy({
							top: overflow,
							behavior: "smooth",
						});
					}
				});
			}

			function animateDetail(details, shouldOpen) {
				const content = detailContent(details);
				rememberDetailOpenState(details, shouldOpen);

				const existingTimer = detailAnimationTimers.get(details);
				if (existingTimer) {
					window.clearTimeout(existingTimer);
					detailAnimationTimers.delete(details);
				}

				if (!content || reducedMotionQuery.matches) {
					details.open = shouldOpen;
					setDetailVisualState(details, shouldOpen);
					return;
				}

				const isFoldPanel = details.classList.contains("fold-panel");
				const animationMs = isFoldPanel ? FOLD_PANEL_ANIMATION_MS : DETAILS_ANIMATION_MS;
				const openingOffset = isFoldPanel ? "translateY(-3px)" : "translateY(-6px)";
				const startHeight = details.open
					? content.getBoundingClientRect().height
					: 0;

				details.open = true;
				setDetailVisualState(details, shouldOpen);
				details.classList.add("is-animating");
				content.style.overflow = "hidden";
				content.style.height = `${startHeight}px`;
				content.style.opacity = shouldOpen ? "0" : "1";
				content.style.transform = shouldOpen ? openingOffset : "translateY(0)";

				const finishTimer = window.setTimeout(() => {
					if (!shouldOpen) {
						details.open = false;
					}
					clearDetailAnimation(details, content);
					if (shouldOpen && !isFoldPanel) {
						scrollOpenedDetailIntoView(details, content);
					}
					detailAnimationTimers.delete(details);
				}, animationMs + 60);

				detailAnimationTimers.set(details, finishTimer);

				window.requestAnimationFrame(() => {
					const endHeight = shouldOpen ? content.scrollHeight : 0;
					content.style.height = `${endHeight}px`;
					content.style.opacity = shouldOpen ? "1" : "0";
					content.style.transform = shouldOpen ? "translateY(0)" : openingOffset;

					if (shouldOpen && !isFoldPanel) {
						scrollOpenedDetailIntoView(details, content);
					}
				});
			}
