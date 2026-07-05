import Foundation
import SwiftUI

struct OperatorLanePopoverView: View {
	let run: OperatorRunStatus
	let currentTime: Date
	@State private var readoutWidth: CGFloat = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			VStack(alignment: .leading, spacing: 3) {
				if let projectTitle {
					measuredReadout {
						OperatorLaneReadoutRow(
							title: "Project",
							items: [OperatorLaneReadoutItem(label: "project", value: projectTitle)]
						)
					}
				}

				measuredReadout {
					OperatorLaneReadoutRow(
						title: "Activity",
						items: [OperatorLaneReadoutItem(label: nil, value: currentSummary)]
					)
				}

				if statusReadoutItems.isEmpty == false {
					measuredReadout {
						OperatorLaneReadoutRow(title: "Status", items: statusReadoutItems)
					}
				}

				if let modelProgress {
					measuredReadout {
						OperatorLaneProgressReadoutRow(
							title: modelProgress.title,
							percent: modelProgress.percent,
							elapsed: modelProgress.elapsed,
							total: modelProgress.total,
							barShare: modelProgress.barShare
						)
					}
				}

				if let continuationRecovery = run.continuationRecovery {
					measuredReadout {
						OperatorLaneRecoveryReadout(recovery: continuationRecovery)
					}
				}
			}

			if modelProgress != nil,
				totalOverviewMetrics.isEmpty == false
					|| detailBuckets.isEmpty == false
					|| lifecycleTableRows.isEmpty == false
			{
				alignedReadout {
					OperatorLaneReadoutDivider()
				}
			}

			VStack(alignment: .leading, spacing: 3) {
				if totalOverviewMetrics.isEmpty == false {
					measuredReadout {
						OperatorTotalMetricsView(metrics: totalOverviewMetrics)
					}
				}

				ForEach(detailBuckets) { bucket in
					measuredReadout {
						OperatorLaneReadoutRow(
							title: rawPanelToken(bucket.name),
							items: bucketReadoutItems(bucket)
						)
					}
				}

				if lifecycleTableRows.isEmpty == false {
					if totalOverviewMetrics.isEmpty == false || detailBuckets.isEmpty == false {
						alignedReadout {
							OperatorLaneReadoutDivider()
						}
					}
					measuredReadout {
						OperatorLifecycleTableView(rows: lifecycleTableRows)
					}
				}

				if detailBuckets.isEmpty,
					totalOverviewMetrics.isEmpty,
					lifecycleTableRows.isEmpty,
					fallbackRunReadoutItems.isEmpty == false
				{
					measuredReadout {
						OperatorLaneReadoutRow(title: "Run", items: fallbackRunReadoutItems)
					}
				}
			}
		}
		.padding(.horizontal, 10)
		.padding(.vertical, 7)
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel("Lane activity for \(run.compactTitle)")
		.onPreferenceChange(OperatorLaneReadoutWidthKey.self) { width in
			guard abs(width - readoutWidth) > 0.5 else {
				return
			}

			readoutWidth = width
		}
	}

	private var alignedWidth: CGFloat? {
		readoutWidth > 0 ? readoutWidth : nil
	}

	private func measuredReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.background(OperatorLaneReadoutWidthReader())
			.frame(width: alignedWidth, alignment: .leading)
	}

	private func alignedReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.frame(width: alignedWidth, alignment: .leading)
	}
}
