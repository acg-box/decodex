import Foundation

struct OperatorLaneReadoutItem: Identifiable {
	let label: String?
	let value: String

	init(label: String?, value: String) {
		self.label = label
		self.value = value
	}

	var id: String {
		"\(label ?? "value")-\(value)"
	}

	var displayValue: String {
		if value.hasSuffix(" tok") {
			return String(value.dropLast(4))
		}

		return value
	}

	var accessibilityText: String {
		if let label {
			return "\(label) \(displayValue)"
		}

		return displayValue
	}

	var summaryRuns: [OperatorLaneReadoutTextRun] {
		switch label?.lowercased() {
		case "wall":
			return [.meta("wall "), .value(displayValue)]
		case "project":
			return [.meta(displayValue)]
		case "attempts":
			return [.value(displayValue), .meta(displayValue == "1" ? " attempt" : " attempts")]
		case "captured":
			return [.value(displayValue), .meta(" captured")]
		case "missing":
			return [.value(displayValue), .meta(" missing")]
		case "events":
			return [.value(displayValue), .meta(" events")]
		case "child events":
			return [.value(displayValue), .meta(" child events")]
		case "input", "input tokens":
			return [.value(displayValue), .meta(" input")]
		case "output", "output tokens":
			return [.value(displayValue), .meta(" output")]
		case "current":
			return [.value(displayValue), .meta(" current")]
		case "current window", "current window tokens":
			return [.value(displayValue), .meta(" current window")]
		case "peak":
			return [.value(displayValue), .meta(" peak")]
		case "peak window", "peak window tokens":
			return [.value(displayValue), .meta(" peak window")]
		case "tools", "tool calls":
			return [.value(displayValue), .meta(" tools")]
		case "output bytes":
			return [.value(displayValue), .meta(" output")]
		case "max output", "max tool output", "largest output":
			return [.value(displayValue), .meta(" max output")]
		case "largest tool", "source":
			return [.value(displayValue), .meta(" source")]
		default:
			if let label {
				return [.meta("\(label) "), .value(displayValue)]
			}
			return [.value(displayValue)]
		}
	}

	func matchesLabel(_ expected: String) -> Bool {
		label?.caseInsensitiveCompare(expected) == .orderedSame
	}
}

enum OperatorLaneReadoutTextRole {
	case meta
	case value
}

struct OperatorLaneReadoutTextRun {
	let text: String
	let role: OperatorLaneReadoutTextRole

	static func meta(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .meta)
	}

	static func value(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .value)
	}
}
