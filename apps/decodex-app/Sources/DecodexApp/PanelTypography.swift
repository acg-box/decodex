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
	static let headerSubtitle = text(11.1, weight: .medium)

	static let emptyIcon = text(16.8, weight: .medium)
	static let emptyTitle = text(12.2, weight: .semibold)
	static let emptyBody = text(10.9, weight: .regular)
	static let accountName = text(12.6, weight: .semibold)
	static let accountDetail = text(10.9, weight: .medium)
	static let metricLabel = text(10.4, weight: .medium)
	static let metricValue = text(11.9, weight: .semibold)
	static let usageLabel = text(10.4, weight: .medium)
	static let usageValue = text(10.7, weight: .semibold)
	static let tertiary = text(9.7, weight: .medium)

	static let iconButton = text(11.2, weight: .semibold)
}
