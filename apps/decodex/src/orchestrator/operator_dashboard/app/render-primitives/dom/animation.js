			function markStableListEnter(node) {
				if (reducedMotionQuery.matches || !(node instanceof HTMLElement)) {
					return;
				}

				node.classList.add("is-list-entering");
				const clear = () => {
					node.classList.remove("is-list-entering");
				};
				node.addEventListener("animationend", clear, { once: true });
				window.setTimeout(clear, 360);
			}

			function animateStableListSize(container, startHeight) {
				if (reducedMotionQuery.matches) {
					return;
				}

				const endHeight = container.getBoundingClientRect().height;
				if (Math.abs(endHeight - startHeight) < 1) {
					return;
				}

				const previousHeight = container.style.height;
				const previousOverflow = container.style.overflow;
				const previousTransition = container.style.transition;
				let cleaned = false;

				const cleanup = (event) => {
					if (event && event.propertyName !== "height") {
						return;
					}
					if (cleaned) {
						return;
					}
					cleaned = true;
					container.classList.remove("is-size-animating");
					container.style.height = previousHeight;
					container.style.overflow = previousOverflow;
					container.style.transition = previousTransition;
				};

				container.classList.add("is-size-animating");
				container.style.height = `${startHeight}px`;
				container.style.overflow = "hidden";
				void container.offsetHeight;

				window.requestAnimationFrame(() => {
					container.style.transition = [previousTransition, "height var(--medium) var(--ease)"]
						.filter(Boolean)
						.join(", ");
					container.style.height = `${endHeight}px`;
					container.addEventListener("transitionend", cleanup, { once: true });
					window.setTimeout(cleanup, 360);
				});
			}
