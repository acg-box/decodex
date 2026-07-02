import Foundation

struct DeviceLoginPrompt: Equatable {
	let verificationURL: URL?
	let userCode: String

	var compactCode: String {
		userCode.filter { character in
			character.isLetter || character.isNumber
		}
	}

	static func parse(_ transcript: String) -> DeviceLoginPrompt? {
		let text = stripANSI(transcript)
		guard let code = parseUserCode(from: text) else {
			return nil
		}

		return DeviceLoginPrompt(
			verificationURL: parseVerificationURL(from: text),
			userCode: code
		)
	}

	private static func parseVerificationURL(from text: String) -> URL? {
		let pattern = #"https?://[^\s\)>\]]+"#
		guard
			let expression = try? NSRegularExpression(pattern: pattern),
			let match = expression.firstMatch(
				in: text,
				range: NSRange(text.startIndex..<text.endIndex, in: text)
			),
			let range = Range(match.range, in: text)
		else {
			return nil
		}

		let value = String(text[range]).trimmingCharacters(in: CharacterSet(charactersIn: ".,;"))
		return URL(string: value)
	}

	private static func parseUserCode(from text: String) -> String? {
		let lines = text.components(separatedBy: .newlines)
		if let codeLine = lineAfterCodePrompt(in: lines) {
			return normalizedCode(from: codeLine)
		}

		for line in lines.reversed() {
			if let code = normalizedCode(from: line) {
				return code
			}
		}

		return nil
	}

	private static func lineAfterCodePrompt(in lines: [String]) -> String? {
		for (index, line) in lines.enumerated() where line.localizedCaseInsensitiveContains("one-time code") {
			for candidate in lines.dropFirst(index + 1) {
				if normalizedCode(from: candidate) != nil {
					return candidate
				}
			}
		}

		return nil
	}

	private static func normalizedCode(from line: String) -> String? {
		let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
		guard trimmed.isEmpty == false else {
			return nil
		}

		let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789- ")
		guard trimmed.uppercased().unicodeScalars.allSatisfy({ allowed.contains($0) }) else {
			return nil
		}

		let compact = trimmed.uppercased().filter { character in
			character.isLetter || character.isNumber
		}
		guard (6...12).contains(compact.count) else {
			return nil
		}

		if compact.count > 4 {
			let split = compact.index(compact.startIndex, offsetBy: 4)
			return "\(compact[..<split])-\(compact[split...])"
		}

		return compact
	}

	private static func stripANSI(_ value: String) -> String {
		guard let expression = try? NSRegularExpression(pattern: "\u{001B}\\[[0-9;]*[A-Za-z]") else {
			return value
		}

		let range = NSRange(value.startIndex..<value.endIndex, in: value)
		return expression.stringByReplacingMatches(
			in: value,
			range: range,
			withTemplate: ""
		)
	}
}
