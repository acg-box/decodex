import Foundation
import SwiftUI

func panelTrimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

struct PanelMetricIconView: View {
	let symbol: String
	let tint: Color

	var body: some View {
		Image(systemName: symbol)
			.font(PanelFont.summaryIcon)
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.frame(width: 12, height: 12)
			.alignmentGuide(.firstTextBaseline) { dimensions in
				dimensions[VerticalAlignment.center] + 3.85
			}
	}
}
