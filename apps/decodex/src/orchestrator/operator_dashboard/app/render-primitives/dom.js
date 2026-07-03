			function renderStableList(container, html) {
				const template = document.createElement("template");
				template.innerHTML = html.trim();
				const startHeight = reducedMotionQuery.matches
					? 0
					: container.getBoundingClientRect().height;

				patchChildNodes(container, template.content, true);
				animateStableListSize(container, startHeight);
			}
