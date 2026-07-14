import AppKit
import ApplicationServices
import CryptoKit

let appName = "Decodex GPUI Spike"
let expectedBundleIdentifier = "space.decodex.gpui-spike"
let expectedWindowTitle = "Decodex GPUI Feasibility"
let expectedLabels = [
	"Decodex workspace feasibility spike",
	"Virtualized conversation history",
	"Message text input",
	"Clear composer",
]
let activationTimeout = 4.0
let pollInterval = 0.1
let activationRetryLimit = 3
let focusRetryLimit = 3
let keyDownInterval = 0.02

struct Expectations {
	let pid: pid_t
	let bundleURL: URL
	let executableURL: URL
	let executableSHA256: String
	let probeURL: URL
	let probeSHA256: String
}

func normalizedFileURL(_ value: String) -> URL {
	URL(fileURLWithPath: value).standardizedFileURL.resolvingSymlinksInPath()
}

func sha256(_ url: URL) -> String? {
	guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
	return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

func parseExpectations() -> Expectations? {
	var values: [String: String] = [:]
	var index = 1
	while index < CommandLine.arguments.count {
		let key = CommandLine.arguments[index]
		guard key.hasPrefix("--"), index + 1 < CommandLine.arguments.count else { return nil }
		values[key] = CommandLine.arguments[index + 1]
		index += 2
	}
	guard values.count == 6,
		let pidValue = values["--expected-pid"],
		let pid = pid_t(pidValue), pid > 0,
		let bundlePath = values["--expected-bundle-url"],
		let executablePath = values["--expected-executable-path"],
		let executableSHA256 = values["--expected-executable-sha256"],
		let probePath = values["--probe-path"],
		let probeSHA256 = values["--expected-probe-sha256"]
	else { return nil }
	return Expectations(
		pid: pid,
		bundleURL: normalizedFileURL(bundlePath),
		executableURL: normalizedFileURL(executablePath),
		executableSHA256: executableSHA256,
		probeURL: normalizedFileURL(probePath),
		probeSHA256: probeSHA256
	)
}

guard let expected = parseExpectations() else {
	fputs("usage: inspect_gpui_spike_accessibility.swift --expected-pid PID --expected-bundle-url PATH --expected-executable-path PATH --expected-executable-sha256 HASH --probe-path PATH --expected-probe-sha256 HASH\n", stderr)
	exit(64)
}

let app = NSRunningApplication(processIdentifier: expected.pid)
let actualBundleIdentifier = app?.bundleIdentifier ?? "missing"
let actualBundleURL = app?.bundleURL?.standardizedFileURL.resolvingSymlinksInPath()
let actualExecutableURL = app?.executableURL?.standardizedFileURL.resolvingSymlinksInPath()
let actualExecutableSHA256 = actualExecutableURL.flatMap(sha256) ?? "missing"
let actualProbeSHA256 = sha256(expected.probeURL) ?? "missing"
let processProvenanceValid = app != nil
	&& app?.isTerminated == false
	&& actualBundleIdentifier == expectedBundleIdentifier
	&& actualBundleURL == expected.bundleURL
	&& actualExecutableURL == expected.executableURL
	&& actualExecutableSHA256 == expected.executableSHA256
	&& actualProbeSHA256 == expected.probeSHA256

if !processProvenanceValid {
	let report: [String: Any] = [
		"process_trusted": AXIsProcessTrusted(),
		"expected_process_identifier": expected.pid,
		"actual_process_identifier": app?.processIdentifier ?? -1,
		"expected_bundle_identifier": expectedBundleIdentifier,
		"actual_bundle_identifier": actualBundleIdentifier,
		"expected_bundle_url": expected.bundleURL.path,
		"actual_bundle_url": actualBundleURL?.path ?? "missing",
		"expected_executable_path": expected.executableURL.path,
		"actual_executable_path": actualExecutableURL?.path ?? "missing",
		"expected_executable_sha256": expected.executableSHA256,
		"actual_executable_sha256": actualExecutableSHA256,
		"probe_path": expected.probeURL.path,
		"expected_probe_sha256": expected.probeSHA256,
		"actual_probe_sha256": actualProbeSHA256,
		"process_provenance_valid": false,
		"ax_inspection_performed": false,
		"application_activated": false,
		"application_activation_attempts": 0,
		"application_and_window_active_at_probe_start": false,
		"role": "not_inspected",
		"title": "not_inspected",
		"window_count": 0,
		"expected_window_title": expectedWindowTitle,
		"actual_window_titles": [],
		"matching_window_count": 0,
		"window_identity_attempts": 0,
		"window_identity_valid": false,
		"visited_elements": 0,
		"roles": [],
		"matched_gpui_labels": [],
		"missing_gpui_labels": expectedLabels,
		"expected_gpui_label_counts": [:],
		"expected_gpui_labels_unique": false,
		"focused_gpui_labels": [],
		"accesskit_activation_attempts": 0,
		"content_activated": false,
		"focus_order": [],
		"keyboard_focus_order": false,
		"accessibility_focus_set_attempts": 0,
		"keyboard_focus_stable_at_dispatch": false,
		"keyboard_focus_preparation_attempts": 0,
		"keyboard_event_attempts": 0,
		"keyboard_value_after_dispatch": "not_inspected",
		"input_role": "not_inspected",
		"input_actions": [],
		"clear_role": "not_inspected",
		"clear_actions": [],
		"accessibility_value_set": false,
		"accessibility_press_cleared_value": false,
		"live_keyboard_input": false,
	]
	let json = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
	print(String(decoding: json, as: UTF8.self))
	exit(2)
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

func label(_ element: AXUIElement) -> String? {
	for name in [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute] {
		if let value = attribute(element, name) as? String, !value.isEmpty {
			return value
		}
	}
	return nil
}

func actions(_ element: AXUIElement) -> [String] {
	var names: CFArray?
	guard AXUIElementCopyActionNames(element, &names) == .success else { return [] }
	return (names as? [String] ?? []).sorted()
}

struct Snapshot {
	var visitedCount: Int
	var roles: Set<String>
	var labels: Set<String>
	var focusedLabels: Set<String>
	var labeledElements: [(String, AXUIElement)]
}

func snapshot(_ root: AXUIElement, _ windows: [AXUIElement]) -> Snapshot {
	var queue = [root] + windows
	var cursor = 0
	var visited = Set<CFHashCode>()
	var roles = Set<String>()
	var labels = Set<String>()
	var focusedLabels = Set<String>()
	var labeledElements: [(String, AXUIElement)] = []

	while cursor < queue.count, visited.count < 5_000 {
		let element = queue[cursor]
		cursor += 1
		let identity = CFHash(element)
		guard visited.insert(identity).inserted else { continue }

		if let value = attribute(element, kAXRoleAttribute) as? String {
			roles.insert(value)
		}
		if let value = label(element) {
			labels.insert(value)
			labeledElements.append((value, element))
			if attribute(element, kAXFocusedAttribute) as? Bool == true {
				focusedLabels.insert(value)
			}
		}
		queue.append(contentsOf: elements(element, kAXChildrenAttribute))
		queue.append(contentsOf: elements(element, kAXRowsAttribute))
	}

	return Snapshot(
		visitedCount: visited.count,
		roles: roles,
		labels: labels,
		focusedLabels: focusedLabels,
		labeledElements: labeledElements
	)
}

func waitUntil(timeout: Double, _ predicate: () -> Bool) -> Bool {
	let deadline = Date().addingTimeInterval(timeout)
	repeat {
		if predicate() { return true }
		Thread.sleep(forTimeInterval: pollInterval)
	} while Date() < deadline
	return false
}

func postUnicode(_ text: String, to pid: pid_t) -> Bool {
	guard let source = CGEventSource(stateID: .combinedSessionState),
		let down = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: true),
		let up = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: false)
	else {
		return false
	}
	let units = Array(text.utf16)
	down.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
	up.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
	down.postToPid(pid)
	Thread.sleep(forTimeInterval: keyDownInterval)
	up.postToPid(pid)
	return true
}

func postKey(_ keyCode: CGKeyCode, flags: CGEventFlags = [], to pid: pid_t) -> Bool {
	guard let source = CGEventSource(stateID: .combinedSessionState),
		let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
		let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false)
	else {
		return false
	}
	down.flags = flags
	up.flags = flags
	down.postToPid(pid)
	up.postToPid(pid)
	return true
}

let root = AXUIElementCreateApplication(expected.pid)
let role = attribute(root, kAXRoleAttribute) as? String ?? "missing"
let title = attribute(root, kAXTitleAttribute) as? String ?? "missing"
var allWindows: [AXUIElement] = []
var matchingWindows: [AXUIElement] = []
var windowIdentityAttempts = 0
let windowIdentityValid = waitUntil(timeout: activationTimeout) {
	windowIdentityAttempts += 1
	allWindows = elements(root, kAXWindowsAttribute)
	matchingWindows = allWindows.filter {
		attribute($0, kAXTitleAttribute) as? String == expectedWindowTitle
	}
	return allWindows.count == 1 && matchingWindows.count == 1
}
let candidateWindow = windowIdentityValid ? matchingWindows[0] : nil

var activationAttempts = 0
var tree = candidateWindow.map { snapshot($0, []) } ?? snapshot(root, [])
let contentActivated = waitUntil(timeout: activationTimeout) {
	activationAttempts += 1
	guard let candidateWindow else { return false }
	tree = snapshot(candidateWindow, [])
	return expectedLabels.allSatisfy(tree.labels.contains)
}

func expectedElement(_ expectedLabel: String) -> AXUIElement? {
	tree.labeledElements.first(where: { $0.0 == expectedLabel })?.1
}

func refreshTree() {
	guard let candidateWindow else { return }
	tree = snapshot(candidateWindow, [])
}

func currentExpectedElement(_ expectedLabel: String) -> AXUIElement? {
	refreshTree()
	return expectedElement(expectedLabel)
}

func applicationAndWindowAreActive() -> Bool {
	app?.isActive == true
		&& candidateWindow.map {
			attribute($0, kAXMainAttribute) as? Bool == true
				&& attribute($0, kAXFocusedAttribute) as? Bool == true
		} == true
}

var applicationActivationAttempts = 0
func ensureApplicationActivated(force: Bool = false) -> Bool {
	if !force, applicationAndWindowAreActive() { return true }
	for _ in 0 ..< activationRetryLimit {
		applicationActivationAttempts += 1
		_ = app?.activate(options: [.activateAllWindows])
		guard let candidateWindow else { continue }
		_ = AXUIElementPerformAction(candidateWindow, kAXRaiseAction as CFString)
		if waitUntil(timeout: 1.0, applicationAndWindowAreActive) { return true }
	}
	return false
}

func expectedElementIsFocused(_ expectedLabel: String) -> Bool {
	guard let element = currentExpectedElement(expectedLabel) else { return false }
	return attribute(element, kAXFocusedAttribute) as? Bool == true
}

var accessibilityFocusSetAttempts = 0
func focusExpectedElement(_ expectedLabel: String) -> Bool {
	for _ in 0 ..< focusRetryLimit {
		guard ensureApplicationActivated() else { continue }
		if expectedElementIsFocused(expectedLabel) { return true }
		guard let element = currentExpectedElement(expectedLabel) else { continue }
		var settable = DarwinBoolean(false)
		guard AXUIElementIsAttributeSettable(
			element,
			kAXFocusedAttribute as CFString,
			&settable
		) == .success, settable.boolValue else { continue }
		accessibilityFocusSetAttempts += 1
		guard AXUIElementSetAttributeValue(
			element,
			kAXFocusedAttribute as CFString,
			kCFBooleanTrue
		) == .success else { continue }
		if waitUntil(timeout: 2.0, { expectedElementIsFocused(expectedLabel) }) {
			return true
		}
	}
	return false
}

var keyboardFocusPreparationAttempts = 0
func establishKeyboardFocusOrder() -> Bool {
	for _ in 0 ..< focusRetryLimit {
		keyboardFocusPreparationAttempts += 1
		guard ensureApplicationActivated(force: true) else { continue }
		guard focusExpectedElement("Clear composer") else { continue }
		guard focusExpectedElement("Message text input") else { continue }
		guard postKey(48, to: expected.pid) else { continue }
		let movedForward = waitUntil(timeout: 2.0) {
			expectedElementIsFocused("Clear composer")
		}
		guard movedForward, postKey(48, flags: .maskShift, to: expected.pid) else {
			continue
		}
		if waitUntil(timeout: 2.0, { expectedElementIsFocused("Message text input") }) {
			return true
		}
	}
	return false
}

let input = expectedElement("Message text input")
let clearButton = expectedElement("Clear composer")
var focusOrder: [String] = []
var keyboardFocusOrder = false
var valueSet = false
var clearPressed = false
var keyboardInput = false
var applicationActivated = false
var keyboardEventAttempts = 0
var keyboardFocusStableAtDispatch = false
var keyboardValueAfterDispatch = "missing"
let actionValue = "Accessibility value path ✓"
let keyboardValue = "Keyboard 日本語 ✓"
let applicationAndWindowActiveAtProbeStart = applicationAndWindowAreActive()

if input != nil, clearButton != nil {
	applicationActivated = ensureApplicationActivated()

	if focusExpectedElement("Message text input") { focusOrder.append("Message text input") }
	if focusExpectedElement("Clear composer") { focusOrder.append("Clear composer") }
	if focusExpectedElement("Message text input") { focusOrder.append("Message text input") }

	for _ in 0 ..< 3 where !valueSet {
		if let currentInput = currentExpectedElement("Message text input"),
			AXUIElementSetAttributeValue(
				currentInput,
				kAXValueAttribute as CFString,
				actionValue as CFString
			) == .success
		{
			valueSet = waitUntil(timeout: 2.0) {
				guard let refreshedInput = currentExpectedElement("Message text input") else { return false }
				return attribute(refreshedInput, kAXValueAttribute) as? String == actionValue
			}
		}
	}
	if let currentClearButton = currentExpectedElement("Clear composer"),
		AXUIElementPerformAction(currentClearButton, kAXPressAction as CFString) == .success
	{
		clearPressed = waitUntil(timeout: 2.0) {
			guard let refreshedInput = currentExpectedElement("Message text input") else { return false }
			return attribute(refreshedInput, kAXValueAttribute) as? String == ""
		}
	}

	keyboardFocusOrder = establishKeyboardFocusOrder()
	keyboardFocusStableAtDispatch = keyboardFocusOrder
		&& applicationAndWindowAreActive()
		&& expectedElementIsFocused("Message text input")
	if keyboardFocusStableAtDispatch {
		keyboardEventAttempts = 1
	}
	if keyboardFocusStableAtDispatch, postUnicode(keyboardValue, to: expected.pid) {
		keyboardInput = waitUntil(timeout: 2.0) {
			guard let refreshedInput = currentExpectedElement("Message text input") else { return false }
			return attribute(refreshedInput, kAXValueAttribute) as? String == keyboardValue
		}
	}
	if let refreshedInput = currentExpectedElement("Message text input") {
		keyboardValueAfterDispatch = attribute(refreshedInput, kAXValueAttribute) as? String ?? "missing"
	}
}

if let candidateWindow {
	tree = snapshot(candidateWindow, [])
}
let missingLabels = expectedLabels.filter { !tree.labels.contains($0) }
let matchedLabels = expectedLabels.filter(tree.labels.contains)
let expectedLabelCounts = Dictionary(uniqueKeysWithValues: expectedLabels.map { expectedLabel in
	(expectedLabel, tree.labeledElements.filter { $0.0 == expectedLabel }.count)
})
let expectedLabelsUnique = expectedLabelCounts.values.allSatisfy { $0 == 1 }
let finalInput = expectedElement("Message text input")
let finalClearButton = expectedElement("Clear composer")
let inputActions = finalInput.map(actions) ?? []
let clearActions = finalClearButton.map(actions) ?? []
let inputRole = finalInput.flatMap { attribute($0, kAXRoleAttribute) as? String } ?? "missing"
let clearRole = finalClearButton.flatMap { attribute($0, kAXRoleAttribute) as? String } ?? "missing"
let hasLabeledTextInput = inputRole == kAXTextFieldRole || inputRole == kAXTextAreaRole
let expectedFocusOrder = ["Message text input", "Clear composer", "Message text input"]

let report: [String: Any] = [
	"process_trusted": AXIsProcessTrusted(),
	"expected_process_identifier": expected.pid,
	"actual_process_identifier": app?.processIdentifier ?? -1,
	"expected_bundle_identifier": expectedBundleIdentifier,
	"actual_bundle_identifier": actualBundleIdentifier,
	"expected_bundle_url": expected.bundleURL.path,
	"actual_bundle_url": actualBundleURL?.path ?? "missing",
	"expected_executable_path": expected.executableURL.path,
	"actual_executable_path": actualExecutableURL?.path ?? "missing",
	"expected_executable_sha256": expected.executableSHA256,
	"actual_executable_sha256": actualExecutableSHA256,
	"probe_path": expected.probeURL.path,
	"expected_probe_sha256": expected.probeSHA256,
	"actual_probe_sha256": actualProbeSHA256,
	"process_provenance_valid": processProvenanceValid,
	"ax_inspection_performed": true,
	"application_activated": applicationActivated,
	"application_activation_attempts": applicationActivationAttempts,
	"application_and_window_active_at_probe_start": applicationAndWindowActiveAtProbeStart,
	"role": role,
	"title": title,
	"window_count": allWindows.count,
	"expected_window_title": expectedWindowTitle,
	"actual_window_titles": allWindows.map {
		attribute($0, kAXTitleAttribute) as? String ?? "missing"
	},
	"matching_window_count": matchingWindows.count,
	"window_identity_attempts": windowIdentityAttempts,
	"window_identity_valid": windowIdentityValid,
	"visited_elements": tree.visitedCount,
	"roles": tree.roles.sorted(),
	"matched_gpui_labels": matchedLabels,
	"missing_gpui_labels": missingLabels,
	"expected_gpui_label_counts": expectedLabelCounts,
	"expected_gpui_labels_unique": expectedLabelsUnique,
	"focused_gpui_labels": tree.focusedLabels.sorted(),
	"accesskit_activation_attempts": activationAttempts,
	"content_activated": contentActivated,
	"focus_order": focusOrder,
	"keyboard_focus_order": keyboardFocusOrder,
	"accessibility_focus_set_attempts": accessibilityFocusSetAttempts,
	"keyboard_focus_stable_at_dispatch": keyboardFocusStableAtDispatch,
	"keyboard_focus_preparation_attempts": keyboardFocusPreparationAttempts,
	"keyboard_event_attempts": keyboardEventAttempts,
	"keyboard_value_after_dispatch": keyboardValueAfterDispatch,
	"input_role": inputRole,
	"input_actions": inputActions,
	"clear_role": clearRole,
	"clear_actions": clearActions,
	"accessibility_value_set": valueSet,
	"accessibility_press_cleared_value": clearPressed,
	"live_keyboard_input": keyboardInput,
]
let json = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
print(String(decoding: json, as: UTF8.self))

guard role == kAXApplicationRole,
	title == appName,
	processProvenanceValid,
	windowIdentityValid,
	contentActivated,
	applicationActivated,
	applicationAndWindowActiveAtProbeStart,
	missingLabels.isEmpty,
	expectedLabelsUnique,
	hasLabeledTextInput,
	clearRole == kAXButtonRole,
	focusOrder == expectedFocusOrder,
	keyboardFocusOrder,
	valueSet,
	clearPressed,
	keyboardInput
else {
	exit(2)
}
