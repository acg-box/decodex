			function keyedPatchNodeKey(node) {
				if (!(node instanceof Element)) {
					return "";
				}

				return node.dataset.renderKey || node.dataset.detailKey || "";
			}

			function syncElementAttributes(current, next) {
				for (const attribute of [...current.attributes]) {
					if (!next.hasAttribute(attribute.name)) {
						current.removeAttribute(attribute.name);
					}
				}

				for (const attribute of [...next.attributes]) {
					if (current.getAttribute(attribute.name) !== attribute.value) {
						current.setAttribute(attribute.name, attribute.value);
					}
				}
			}

			function patchNode(current, next) {
				if (
					current.nodeType !== next.nodeType ||
					current.nodeName !== next.nodeName
				) {
					current.replaceWith(next.cloneNode(true));
					return;
				}

				if (current.nodeType === Node.TEXT_NODE) {
					if (current.nodeValue !== next.nodeValue) {
						current.nodeValue = next.nodeValue;
					}
					return;
				}

				if (!(current instanceof Element) || !(next instanceof Element)) {
					return;
				}

				// Preserve active accordion animation styles until their timer clears them.
				if (
					current.closest("details.is-animating") &&
					!(current instanceof HTMLDetailsElement)
				) {
					return;
				}

				if (current instanceof HTMLDetailsElement) {
					const detailKey = detailStateKey(current);
					if (detailKey && detailDisclosureState.has(detailKey)) {
						const shouldOpen = detailDisclosureState.get(detailKey);
						if (shouldOpen) {
							next.setAttribute("open", "");
							next.dataset.detailState = "open";
						} else {
							next.removeAttribute("open");
							delete next.dataset.detailState;
						}
					}
				}

				syncElementAttributes(current, next);
				patchChildNodes(current, next, false);
			}

			function patchChildNodes(current, next, animateInsertions = false) {
				const currentChildren = [...current.childNodes];
				const nextChildren = [...next.childNodes];
				const keyedCurrent = new Map();

				for (const child of currentChildren) {
					const key = keyedPatchNodeKey(child);
					if (key && !keyedCurrent.has(key)) {
						keyedCurrent.set(key, child);
					}
				}

				let cursor = current.firstChild;
				const used = new Set();

				for (const nextChild of nextChildren) {
					const key = keyedPatchNodeKey(nextChild);
					let currentChild = key ? keyedCurrent.get(key) : null;

					while (cursor && used.has(cursor)) {
						cursor = cursor.nextSibling;
					}

					if (!currentChild && cursor && !keyedPatchNodeKey(cursor)) {
						currentChild = cursor;
					}

					if (currentChild) {
						used.add(currentChild);
						patchNode(currentChild, nextChild);
						if (currentChild !== cursor) {
							current.insertBefore(currentChild, cursor);
						}
						cursor = currentChild.nextSibling;
					} else {
						const clone = nextChild.cloneNode(true);
						current.insertBefore(clone, cursor);
						if (animateInsertions) {
							markStableListEnter(clone);
						}
						used.add(clone);
					}
				}

				for (const child of [...current.childNodes]) {
					if (!used.has(child)) {
						child.remove();
					}
				}
			}
