import SwiftUI

enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerTitle = text(15.4, weight: .semibold)
	static let headerSubtitle = text(10.8, weight: .regular)

	static let emptyIcon = text(17, weight: .medium)
	static let emptyTitle = text(12.5, weight: .semibold)
	static let emptyBody = text(11, weight: .regular)
	static let accountName = text(12.4, weight: .semibold)
	static let accountDetail = text(10.6, weight: .regular)
	static let metricLabel = text(9.8, weight: .medium)
	static let metricValue = text(10.8, weight: .semibold)
	static let usageLabel = text(9.8, weight: .regular)
	static let usageValue = text(10.4, weight: .medium)
	static let tertiary = text(9.5, weight: .regular)
	static let compactAction = text(9.8, weight: .medium)

	static let iconButton = text(11, weight: .medium)
}
