import SwiftUI

struct OperatorLaneHeaderReadoutView: View {
	let status: String
	let project: String?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 8) {
			Text(status)
				.font(OperatorLanePopoverStyle.titleFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.tail)
				.fixedSize(horizontal: true, vertical: false)

			if let project = panelTrimmed(project) {
				Text(project)
					.font(OperatorLanePopoverStyle.projectFont)
					.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
					.lineLimit(1)
					.fixedSize(horizontal: true, vertical: false)
					.help(project)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}
