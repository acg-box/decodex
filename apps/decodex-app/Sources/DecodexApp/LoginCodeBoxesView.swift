import SwiftUI

struct LoginCodeBoxesView: View {
	let code: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 4) {
			ForEach(0..<boxCount, id: \.self) { index in
				if index == 4 {
					Spacer()
						.frame(width: 2)
				}

				Text(character(at: index))
					.font(LoginFont.code)
					.monospacedDigit()
					.foregroundStyle(foreground)
					.frame(width: 23, height: 31)
					.background {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.fill(LoginPalette.codeBoxFill(colorScheme))
					}
					.overlay {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.strokeBorder(LoginPalette.codeBoxStroke(colorScheme), lineWidth: 0.75)
							.allowsHitTesting(false)
					}
					.shadow(
						color: LoginPalette.codeBoxShadow(colorScheme),
						radius: colorScheme == .dark ? 2.5 : 1.4,
						y: 0.7
					)
			}
		}
		.frame(maxWidth: .infinity, alignment: .center)
	}

	private var characters: [String] {
		code.map { String($0).uppercased() }
	}

	private var boxCount: Int {
		max(8, min(12, characters.count))
	}

	private var foreground: Color {
		if code.isEmpty {
			return LoginPalette.secondaryText(colorScheme).opacity(0.42)
		}

		return LoginPalette.primaryText(colorScheme)
	}

	private func character(at index: Int) -> String {
		guard characters.indices.contains(index) else {
			return ""
		}

		return characters[index]
	}
}
