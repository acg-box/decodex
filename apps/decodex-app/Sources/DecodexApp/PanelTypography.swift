import SwiftUI

enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerTitle = text(14.2, weight: .semibold)

	static let emptyIcon = text(17, weight: .medium)
	static let emptyTitle = text(12.5, weight: .semibold)
	static let emptyBody = text(11, weight: .regular)
	static let transientTitle = emptyTitle
	static let transientBody = emptyBody
	static let accountName = text(12.4, weight: .semibold)
	static let accountDetail = text(10.6, weight: .regular)
	static let loginCode = text(19, weight: .semibold, design: .monospaced)
	static let usageLabel = text(9.8, weight: .regular)
	static let usageValue = text(10.4, weight: .medium)
	static let resetCardAction = text(9.4, weight: .medium)
	static let quotaText = text(10.4, weight: .regular)
	static let tertiary = text(9.5, weight: .regular)
	static let compactAction = text(9.8, weight: .medium)

	static let iconButton = text(11, weight: .medium)
}
