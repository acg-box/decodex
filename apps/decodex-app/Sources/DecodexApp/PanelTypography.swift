import SwiftUI

enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerTitle = text(14.8, weight: .semibold)
	static let headerSubtitle = text(10.5, weight: .regular)

	static let emptyIcon = text(16.8, weight: .medium)
	static let emptyTitle = text(12.2, weight: .semibold)
	static let emptyBody = text(10.9, weight: .regular)
	static let accountName = text(11.7, weight: .semibold)
	static let accountDetail = text(10.2, weight: .regular)
	static let metricLabel = text(9.6, weight: .medium)
	static let metricValue = text(10.7, weight: .semibold)
	static let usageLabel = text(9.6, weight: .regular)
	static let usageValue = text(10.1, weight: .medium)
	static let tertiary = text(9.2, weight: .regular)
	static let compactAction = text(9.3, weight: .medium)

	static let iconButton = text(10.6, weight: .medium)
}
