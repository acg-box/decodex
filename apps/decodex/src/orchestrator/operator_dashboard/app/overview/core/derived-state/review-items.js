			function addPostReviewLaneDerivedItems(
				postReviewLanes,
				currentLaneByIssue,
				waitingItems,
				readyItems,
				attentionItems,
				cleanupIssueKeys,
			) {
				for (const lane of postReviewLanes) {
					const tone = toneForLane(lane);
					const currentLane = currentLaneForIssue(lane, currentLaneByIssue);
					const shadowedByCurrentLane = lane.shadowed_by_current_lane === true;
					const issueKey = issueDisplayKey(lane);

					if (shadowedByCurrentLane) {
						waitingItems.push(shadowedReviewWaitingItem(lane, currentLane, issueKey));
						continue;
					}

					if (isPostReviewBlocker(lane)) {
						const blockerScope = postReviewBlockerScope(lane);
						if (blockerScope === "Cleanup") {
							cleanupIssueKeys.add(issueKey);
						}
						attentionItems.push(postReviewBlockerItem(lane, blockerScope, tone, issueKey));
						continue;
					}

					if (lane.classification === "wait_for_review") {
						waitingItems.push(postReviewWaitingItem(lane, tone, issueKey));
						continue;
					}

					if (lane.classification === "ready_to_land") {
						readyItems.push(postReviewReadyItem(lane, tone, issueKey));
					}
				}
			}

			function currentLaneForIssue(lane, currentLaneByIssue) {
				return issueIdentityKeys(lane)
					.map((key) => currentLaneByIssue.get(key))
					.find(Boolean);
			}

			function shadowedReviewWaitingItem(lane, currentLane, issueKey) {
				const currentLaneFacts = currentLane
					? [
						["Run", currentLane.run_id],
						[
							"Operation",
							displayToken(currentLane.current_operation || currentLane.run_phase || currentLane.phase),
						],
					]
					: [["Run", "active"]];

				return {
					tone: "tone-run",
					scope: "Review",
					issue: issueKey,
					title: "Repair running",
					summary: "",
					status: currentLane
						? `run phase ${displayToken(currentLane.run_phase || currentLane.phase)}`
						: "current lane",
					facts: [
						...currentLaneFacts,
						["Checks", compactStateToken(lane.check_state)],
						["Threads", reviewThreadToken(lane.unresolved_review_threads)],
						["PR", optionalCardToken(lane.pr_url)],
						["Branch", optionalCardToken(lane.branch_name)],
						...loopStatusFacts(lane.loop_status),
						...postReviewReadbackFacts(lane),
					],
				};
			}

			function postReviewBlockerItem(lane, blockerScope, tone, issueKey) {
				return {
					tone,
					scope: blockerScope,
					issue: issueKey,
					title: postReviewBlockerTitle(lane),
					summary: "",
					status: postReviewBlockerStatus(lane, blockerScope),
					facts: [
						["Checks", compactStateToken(lane.check_state)],
						["Threads", reviewThreadToken(lane.unresolved_review_threads)],
						["PR", optionalCardToken(lane.pr_url)],
						["Branch", optionalCardToken(lane.branch_name)],
						...loopStatusFacts(lane.loop_status),
						...postReviewReadbackFacts(lane),
					],
				};
			}

			function postReviewWaitingItem(lane, tone, issueKey) {
				return {
					tone,
					scope: "Review",
					issue: issueKey,
					title: "Wait for review",
					summary: "",
							status: lane.check_state ? `checks ${compactStateToken(lane.check_state)}` : "waiting",
					facts: [
						["Review decision", compactStateToken(lane.review_decision)],
						["Threads", reviewThreadToken(lane.unresolved_review_threads)],
						["PR", optionalCardToken(lane.pr_url)],
						["Branch", optionalCardToken(lane.branch_name)],
						...loopStatusFacts(lane.loop_status),
						...postReviewReadbackFacts(lane),
					],
				};
			}

			function postReviewReadyItem(lane, tone, issueKey) {
				return {
					tone,
					scope: "Review",
					issue: issueKey,
					title: "Ready to land",
					summary: "",
							status: lane.mergeable ? `merge ${compactStateToken(lane.mergeable)}` : "ready",
					facts: [
						["Review decision", compactStateToken(lane.review_decision)],
						["Checks", compactStateToken(lane.check_state)],
						["Threads", reviewThreadToken(lane.unresolved_review_threads)],
						["PR", optionalCardToken(lane.pr_url)],
						["Branch", optionalCardToken(lane.branch_name)],
						...loopStatusFacts(lane.loop_status),
						...postReviewReadbackFacts(lane),
					],
				};
			}
