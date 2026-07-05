

				function renderCodexAccountPoolRow(account, snapshot, isLastAccount = false) {
					const weight = codexAccountCapacityLabel(account);
					const statusTone = codexAccountStatusTone(account);
					const toneClass = statusTone ? ` is-${statusTone}` : "";
					const fixed = codexAccountMatchesConfiguredSelector(account, snapshot);
					const fixedClass = fixed ? " is-fixed" : "";
					const selector = codexAccountControlSelector(account);
					const action = fixed ? "clearAccountSelection" : "selectAccount";
					const armedClass =
						selector && accountSelectionConfirmationMatches(action, selector) ? " is-armed" : "";
					const selectedClass =
						String(account.status || "").toLowerCase() === "selected" ? " is-selected" : "";
					const metaFacts = codexAccountMetaFacts(account);
					const credits = codexAccountCreditsSummary(account);
					const resetCredits = codexAccountResetCreditsSummary(account);
					const creditValue = credits || "-";
					const creditTitle = [credits ? `credits ${credits}` : "credits unavailable", resetCredits ? `reset cards ${resetCredits}` : ""]
						.filter(Boolean)
						.join(", ");
					const creditTone = codexAccountCreditsTone(account);
					const creditClass = creditTone ? ` is-${creditTone}` : "";
					const identityClass = codexAccountShowsEmail(account) ? " is-machine" : "";
					const profileKey = codexAccountProfileExpansionKey(account);
					const hasProfileDetails = codexAccountHasProfileDetails(account);
					const profileExpanded = hasProfileDetails && codexAccountProfileExpanded(account);
					const profileOpenClass = profileExpanded ? " is-profile-open" : "";
					const profileToggleableClass = hasProfileDetails ? " is-profile-toggleable" : "";
					const lastAccountClass = isLastAccount ? " is-last-account" : "";
					const profileRowToggleAttribute = hasProfileDetails
						? ` data-account-profile-row-toggle="${escapeHtml(profileKey)}"`
						: "";

					return `
							<div class="account-row${selectedClass}${fixedClass}${armedClass}${toneClass}${profileOpenClass}${profileToggleableClass}${lastAccountClass}" data-render-key="account-row:${escapeHtml(profileKey)}"${profileRowToggleAttribute}>
								<div class="account-row-id${identityClass}">
									${renderCodexAccountNameControl(account, snapshot)}
									${renderCodexAccountRandomNameButton(account)}
								</div>
								<div class="account-row-plan">${escapeHtml(weight)}</div>
								${renderCodexAccountPoolWindow(account, "primary")}
								${renderCodexAccountPoolWindow(account, "secondary")}
								<div class="account-row-credit${creditClass}" title="${escapeHtml(creditTitle)}">
									<strong>${escapeHtml(creditValue)}</strong>
									${resetCredits ? `<small class="account-row-reset-credit">${escapeHtml(resetCredits)}</small>` : ""}
								</div>
								<div class="account-row-state">
									<strong class="account-status">${escapeHtml(codexAccountStatusLabel(account))}</strong>
								</div>
								${
									metaFacts.length
										? `<div class="account-meta account-row-meta">
												${metaFacts
													.map(
														([label, value]) =>
															`<span>${escapeHtml(label)} <strong>${escapeHtml(value)}</strong></span>`,
													)
													.join("")}
											</div>`
										: ""
								}
								${hasProfileDetails ? renderCodexAccountProfileToggle(account, profileExpanded) : ""}
							</div>
							${hasProfileDetails ? renderCodexAccountProfilePanel(account, snapshot, profileKey, profileExpanded) : ""}
							`;
				}

				function codexAccountProfileFactMap(account) {
					return new Map(codexAccountProfileMetaFacts(account));
				}

				function renderCodexAccountProfilePanel(account, snapshot, profileKey, expanded) {
					const facts = codexAccountProfileFactMap(account);
					const activity = renderCodexAccountProfileActivityStrip(account);
					const resetCredits = renderCodexAccountResetCreditsStrip(account);
					if (!facts.size && !activity && !resetCredits) {
						return "";
					}

					const statusTone = codexAccountStatusTone(account);
					const toneClass = statusTone ? ` is-${statusTone}` : "";
					const fixedClass = codexAccountMatchesConfiguredSelector(account, snapshot)
						? " is-fixed"
						: "";
					const openClass = expanded ? " is-open" : "";
					const selectedClass =
						String(account.status || "").toLowerCase() === "selected" ? " is-selected" : "";
					const title = [
						codexAccountDisplayTitle(account),
						...Array.from(facts, ([label, value]) => `${label} ${value}`),
					]
						.filter(Boolean)
						.join(" · ");
					const metricFacts = [
						["Lifetime", facts.get("tok") || "-"],
						["Peak day", facts.get("peak") || "-"],
						["Streak", facts.get("streak") || "-"],
						["Longest task", facts.get("task") || "-"],
					];

					return `
						<div class="account-profile-panel${selectedClass}${fixedClass}${toneClass}${openClass}" data-render-key="account-profile-panel:${escapeHtml(profileKey)}" aria-hidden="${expanded ? "false" : "true"}" title="${escapeHtml(title)}">
							<div class="account-profile-panel-inner">
								${metricFacts
									.map(
										([label, value]) => `
											<div class="account-profile-fact">
												<span>${escapeHtml(label)}</span>
												<strong>${escapeHtml(value)}</strong>
											</div>
										`,
									)
									.join("")}
								<div class="account-profile-activity is-reset-cards">
									<span class="account-profile-activity-label">Reset cards</span>
									${resetCredits || '<strong class="account-profile-empty">-</strong>'}
								</div>
								<div class="account-profile-activity">
									<span class="account-profile-activity-label">Activity</span>
									${activity || '<strong class="account-profile-empty">-</strong>'}
								</div>
							</div>
						</div>
					`;
				}
