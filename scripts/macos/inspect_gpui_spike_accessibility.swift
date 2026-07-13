import AppKit
import ApplicationServices

let appName = "Decodex GPUI Spike"
let expectedLabels = [
	"Decodex workspace feasibility spike",
	"Virtualized conversation history",
	"Message text input",
]

guard let app = NSWorkspace.shared.runningApplications.first(where: { $0.localizedName == appName }) else {
	fputs("Decodex GPUI Spike is not running\n", stderr)
	exit(1)
}

func attribute(_ element: AXUIElement, _ name: String) -> Any? {
	var value: CFTypeRef?
	guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
		return nil
	}
	return value
}

func elements(_ element: AXUIElement, _ name: String) -> [AXUIElement] {
	attribute(element, name) as? [AXUIElement] ?? []
}

let root = AXUIElementCreateApplication(app.processIdentifier)
let role = attribute(root, kAXRoleAttribute) as? String ?? "missing"
let title = attribute(root, kAXTitleAttribute) as? String ?? "missing"
let windows = elements(root, kAXWindowsAttribute)

var queue = [root] + windows
var cursor = 0
var visited = Set<CFHashCode>()
var roles = Set<String>()
var labels = Set<String>()
var focusedLabels = Set<String>()

while cursor < queue.count, visited.count < 5_000 {
	let element = queue[cursor]
	cursor += 1
	let identity = CFHash(element)
	guard visited.insert(identity).inserted else { continue }

	if let value = attribute(element, kAXRoleAttribute) as? String {
		roles.insert(value)
	}
	for name in [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute] {
		if let value = attribute(element, name) as? String, !value.isEmpty {
			labels.insert(value)
			if attribute(element, kAXFocusedAttribute) as? Bool == true {
				focusedLabels.insert(value)
			}
		}
	}
	queue.append(contentsOf: elements(element, kAXChildrenAttribute))
	queue.append(contentsOf: elements(element, kAXRowsAttribute))
}

let missingLabels = expectedLabels.filter { !labels.contains($0) }
let matchedLabels = expectedLabels.filter { labels.contains($0) }
let focusedExpectedLabels = expectedLabels.filter { focusedLabels.contains($0) }
let report: [String: Any] = [
	"process_trusted": AXIsProcessTrusted(),
	"role": role,
	"title": title,
	"window_count": windows.count,
	"visited_elements": visited.count,
	"roles": roles.sorted(),
	"matched_gpui_labels": matchedLabels,
	"focused_gpui_labels": focusedExpectedLabels,
	"missing_gpui_labels": missingLabels,
]
let json = try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys])
print(String(decoding: json, as: UTF8.self))

let hasTextInput = roles.contains(kAXTextFieldRole) || roles.contains(kAXTextAreaRole)
guard role == kAXApplicationRole,
	title == appName,
	!windows.isEmpty,
	missingLabels.isEmpty,
	hasTextInput,
	!focusedExpectedLabels.isEmpty
else {
	exit(2)
}
