import SwiftUI

struct PanelWindowContentSizeKey: PreferenceKey {
	static let defaultValue = CGSize.zero

	static func reduce(value: inout CGSize, nextValue: () -> CGSize) {
		let next = nextValue()
		guard next != .zero else {
			return
		}
		value = next
	}
}
