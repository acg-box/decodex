#!/usr/bin/env swift

import AppKit
import ApplicationServices
import CryptoKit
import Darwin
import Foundation

let expectedBundleIdentifier = "box.acg.decodex"
let expectedWindowTitle = "Decodex"
let expectedShellLabel = "Decodex operational shell"
let destinations = ["Factory", "Workbench", "Accounts", "Health"]
let focusOrder = destinations + ["Open settings"]
let axMessagingTimeout: Float = 1.0
let phasePollInterval = 0.05
let maximumTreeNodes = 256

enum DiagnosticFailure: Error, CustomStringConvertible {
	case message(String)

	var description: String {
		if case let .message(value) = self { value } else { "unknown diagnostic failure" }
	}
}

struct Expectations {
	let pid: pid_t
	let bundleURL: URL
	let executableURL: URL
	let executableSHA256: String
	let sourceURL: URL
	let sourceSHA256: String
	let inspectorURL: URL
	let inspectorSHA256: String
	let journalURL: URL
	let reportURL: URL
	let screenshotURL: URL
}

final class Journal {
	private let handle: FileHandle
	private var sequence = 0

	init(url: URL) throws {
		guard FileManager.default.createFile(atPath: url.path, contents: nil) else {
			throw DiagnosticFailure.message("cannot create phase journal")
		}
		handle = try FileHandle(forWritingTo: url)
	}

	deinit { try? handle.close() }

	func append(_ fields: [String: Any]) throws {
		sequence += 1
		var record = fields
		record["sequence"] = sequence
		record["timestamp"] = ISO8601DateFormatter().string(from: Date())
		guard JSONSerialization.isValidJSONObject(record) else {
			throw DiagnosticFailure.message("invalid journal record")
		}
		var data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
		data.append(0x0a)
		try handle.write(contentsOf: data)
		try handle.synchronize()
		guard Darwin.fsync(handle.fileDescriptor) == 0 else {
			throw DiagnosticFailure.message("phase journal fsync failed: \(errno)")
		}
	}
}

struct ElementFact {
	let element: AXUIElement
	let label: String?
	let role: String
	let nativeValue: Any?
	let focused: Bool
}

struct TreeSnapshot {
	let visited: Int
	let facts: [ElementFact]
	let roles: Set<String>
}

func normalized(_ value: String) -> URL {
	URL(fileURLWithPath: value).standardizedFileURL.resolvingSymlinksInPath()
}

func sha256(_ url: URL) -> String? {
	guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
	return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

func rawArgument(_ name: String) -> String? {
	guard let index = CommandLine.arguments.firstIndex(of: name), index + 1 < CommandLine.arguments.count
	else { return nil }
	return CommandLine.arguments[index + 1]
}

func parseExpectations() throws -> Expectations {
	var values: [String: String] = [:]
	var index = 1
	while index < CommandLine.arguments.count {
		guard index + 1 < CommandLine.arguments.count else {
			throw DiagnosticFailure.message("missing value for \(CommandLine.arguments[index])")
		}
		values[CommandLine.arguments[index]] = CommandLine.arguments[index + 1]
		index += 2
	}
	let expectedKeys = Set([
		"--expected-pid", "--expected-bundle-url", "--expected-executable-path",
		"--expected-executable-sha256", "--inspector-source-path",
		"--expected-inspector-source-sha256", "--inspector-executable-path",
		"--expected-inspector-executable-sha256", "--journal-path", "--report-path",
		"--screenshot-path",
	])
	guard Set(values.keys) == expectedKeys,
		let pidValue = values["--expected-pid"], let pid = pid_t(pidValue), pid > 0,
		let bundle = values["--expected-bundle-url"],
		let executable = values["--expected-executable-path"],
		let executableHash = values["--expected-executable-sha256"],
		let source = values["--inspector-source-path"],
		let sourceHash = values["--expected-inspector-source-sha256"],
		let inspector = values["--inspector-executable-path"],
		let inspectorHash = values["--expected-inspector-executable-sha256"],
		let journal = values["--journal-path"], let report = values["--report-path"],
		let screenshot = values["--screenshot-path"]
	else { throw DiagnosticFailure.message("invalid inspector arguments") }
	return Expectations(
		pid: pid,
		bundleURL: normalized(bundle),
		executableURL: normalized(executable),
		executableSHA256: executableHash,
		sourceURL: normalized(source),
		sourceSHA256: sourceHash,
		inspectorURL: normalized(inspector),
		inspectorSHA256: inspectorHash,
		journalURL: normalized(journal),
		reportURL: normalized(report),
		screenshotURL: normalized(screenshot)
	)
}

func processPath(_ pid: pid_t) -> String {
	var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN) * 4)
	let length = proc_pidpath(pid, &buffer, UInt32(buffer.count))
	guard length > 0 else { return "unavailable" }
	return String(cString: buffer)
}

func jsonValue(_ value: Any?) -> Any {
	switch value {
	case let string as String: string
	case let number as NSNumber: number
	case let boolean as Bool: boolean
	case nil: NSNull()
	default: String(describing: value)
	}
}

func writeJSON(_ value: Any, to url: URL) throws {
	guard JSONSerialization.isValidJSONObject(value) else {
		throw DiagnosticFailure.message("invalid diagnostic report")
	}
	let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
	try data.write(to: url, options: .atomic)
	let handle = try FileHandle(forWritingTo: url)
	defer { try? handle.close() }
	try handle.synchronize()
	_ = Darwin.fsync(handle.fileDescriptor)
}

final class AXRecorder {
	let journal: Journal
	private(set) var operationCount = 0

	init(journal: Journal) { self.journal = journal }

	func copy(
		_ element: AXUIElement,
		_ attribute: String,
		operation: String
	) throws -> (AXError, CFTypeRef?) {
		let started = Date()
		var value: CFTypeRef?
		let error = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
		operationCount += 1
		try journal.append([
			"event": "ax_operation",
			"operation": operation,
			"attribute": attribute,
			"ax_error": error.rawValue,
			"elapsed_ms": Date().timeIntervalSince(started) * 1_000,
		])
		return (error, value)
	}

}

func phase<T>(
	_ name: String,
	journal: Journal,
	results: inout [String: Any],
	_ body: () throws -> (T, [String: Any])
) throws -> T {
	let started = Date()
	try journal.append(["event": "phase_started", "phase": name])
	do {
		let (value, details) = try body()
		var receipt = details
		receipt["passed"] = true
		receipt["elapsed_ms"] = Date().timeIntervalSince(started) * 1_000
		results[name] = receipt
		try journal.append(["event": "phase_completed", "phase": name, "receipt": receipt])
		return value
	} catch {
		let receipt: [String: Any] = [
			"passed": false,
			"elapsed_ms": Date().timeIntervalSince(started) * 1_000,
			"error": String(describing: error),
		]
		results[name] = receipt
		try? journal.append(["event": "phase_failed", "phase": name, "receipt": receipt])
		throw error
	}
}

func label(
	_ element: AXUIElement,
	prefix: String,
	recorder: AXRecorder
) throws -> String? {
	for (suffix, attribute) in [
		("title", kAXTitleAttribute),
		("description", kAXDescriptionAttribute),
		("value", kAXValueAttribute),
	] {
		let (_, value) = try recorder.copy(element, attribute, operation: "\(prefix).\(suffix)")
		if let string = value as? String, !string.isEmpty { return string }
	}
	return nil
}

func snapshot(_ root: AXUIElement, recorder: AXRecorder) throws -> TreeSnapshot {
	var queue = [root]
	var cursor = 0
	var visited = Set<CFHashCode>()
	var facts: [ElementFact] = []
	var roles = Set<String>()
	while cursor < queue.count, visited.count < maximumTreeNodes {
		let element = queue[cursor]
		cursor += 1
		guard visited.insert(CFHash(element)).inserted else { continue }
		let prefix = "tree.node.\(visited.count)"
		let (_, roleValue) = try recorder.copy(element, kAXRoleAttribute, operation: "\(prefix).role")
		let role = roleValue as? String ?? "missing"
		let elementLabel = try label(element, prefix: prefix, recorder: recorder)
		let (_, nativeValue) = try recorder.copy(
			element, kAXValueAttribute, operation: "\(prefix).native_value"
		)
		let (_, focusedValue) = try recorder.copy(
			element, kAXFocusedAttribute, operation: "\(prefix).focused"
		)
		facts.append(ElementFact(
			element: element,
			label: elementLabel,
			role: role,
			nativeValue: nativeValue,
			focused: focusedValue as? Bool == true
		))
		roles.insert(role)
		let (_, childrenValue) = try recorder.copy(
			element, kAXChildrenAttribute, operation: "\(prefix).children"
		)
		if let children = childrenValue as? [AXUIElement] {
			queue.append(contentsOf: children.prefix(maximumTreeNodes - visited.count))
		}
	}
	guard cursor >= queue.count else {
		throw DiagnosticFailure.message("Accessibility tree exceeded \(maximumTreeNodes) nodes")
	}
	return TreeSnapshot(visited: visited.count, facts: facts, roles: roles)
}

func labelMatches(_ actual: String?, _ expected: String) -> Bool {
	guard let actual else { return false }
	return actual == expected || actual.hasPrefix("\(expected):")
}

func fact(_ expectedLabel: String, in tree: TreeSnapshot) throws -> ElementFact {
	let matches = tree.facts.filter { labelMatches($0.label, expectedLabel) }
	guard matches.count == 1, let match = matches.first else {
		throw DiagnosticFailure.message("expected one \(expectedLabel) element, found \(matches.count)")
	}
	return match
}

func focusedFact(root: AXUIElement, recorder: AXRecorder, operation: String) throws -> ElementFact {
	let (error, value) = try recorder.copy(
		root, kAXFocusedUIElementAttribute, operation: "\(operation).focused_ui_element"
	)
	guard error == .success, let value, CFGetTypeID(value) == AXUIElementGetTypeID() else {
		throw DiagnosticFailure.message("focused element readback failed with AXError \(error.rawValue)")
	}
	let element = unsafeBitCast(value, to: AXUIElement.self)
	let elementLabel = try label(element, prefix: operation, recorder: recorder)
	let (_, roleValue) = try recorder.copy(element, kAXRoleAttribute, operation: "\(operation).role")
	let (_, nativeValue) = try recorder.copy(
		element, kAXValueAttribute, operation: "\(operation).native_value"
	)
	return ElementFact(
		element: element,
		label: elementLabel,
		role: roleValue as? String ?? "missing",
		nativeValue: nativeValue,
		focused: true
	)
}

func waitForFocused(
	_ expectedLabel: String,
	root: AXUIElement,
	recorder: AXRecorder,
	operation: String
) throws -> ElementFact {
	let deadline = Date().addingTimeInterval(2.0)
	repeat {
		if let current = try? focusedFact(root: root, recorder: recorder, operation: operation),
			labelMatches(current.label, expectedLabel)
		{
			return current
		}
		Thread.sleep(forTimeInterval: phasePollInterval)
	} while Date() < deadline
	let current = try focusedFact(root: root, recorder: recorder, operation: "\(operation).final")
	throw DiagnosticFailure.message(
		"focused element is \(current.label ?? "missing"), expected \(expectedLabel)"
	)
}

func waitForKeyboardBaseline(
	root: AXUIElement,
	recorder: AXRecorder
) throws -> ElementFact {
	let deadline = Date().addingTimeInterval(2.0)
	repeat {
		if let current = try? focusedFact(
			root: root, recorder: recorder, operation: "keyboard_baseline.readback"
		), current.label == expectedShellLabel || labelMatches(current.label, destinations[0]) {
			return current
		}
		Thread.sleep(forTimeInterval: phasePollInterval)
	} while Date() < deadline
	let current = try focusedFact(
		root: root, recorder: recorder, operation: "keyboard_baseline.final"
	)
	throw DiagnosticFailure.message(
		"keyboard baseline is \(current.label ?? "missing"), expected shell or Factory"
	)
}

func waitForNativeBool(
	_ expectedValue: Bool,
	element: AXUIElement,
	recorder: AXRecorder,
	operation: String
) throws -> Bool {
	let deadline = Date().addingTimeInterval(2.0)
	repeat {
		let (error, value) = try recorder.copy(
			element, kAXValueAttribute, operation: "\(operation).native_value"
		)
		if error == .success, value as? Bool == expectedValue { return expectedValue }
		Thread.sleep(forTimeInterval: phasePollInterval)
	} while Date() < deadline
	throw DiagnosticFailure.message("native AXValue did not become \(expectedValue)")
}

func postKey(
	_ keyCode: CGKeyCode,
	flags: CGEventFlags = [],
	to pid: pid_t,
	interval: Double
) throws -> Double {
	guard let source = CGEventSource(stateID: .combinedSessionState),
		let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
		let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false)
	else { throw DiagnosticFailure.message("cannot construct keyboard event") }
	down.flags = flags
	up.flags = flags
	let started = Date()
	down.postToPid(pid)
	Thread.sleep(forTimeInterval: interval)
	up.postToPid(pid)
	return Date().timeIntervalSince(started) * 1_000
}

func keyboardStep(
	operation: String,
	expectedLabel: String,
	keyCode: CGKeyCode,
	flags: CGEventFlags = [],
	pid: pid_t,
	root: AXUIElement,
	recorder: AXRecorder,
	journal: Journal
) throws -> [String: Any] {
	let interval = 0.04
	let dispatchElapsed = try postKey(keyCode, flags: flags, to: pid, interval: interval)
	try journal.append([
		"event": "input_operation",
		"operation": operation,
		"key_code": keyCode,
		"flags_raw_value": flags.rawValue,
		"down_up_interval_ms": interval * 1_000,
		"elapsed_ms": dispatchElapsed,
	])
	let focused = try waitForFocused(
		expectedLabel, root: root, recorder: recorder, operation: "\(operation).readback"
	)
	return [
		"expected_label": expectedLabel,
		"focused_label": focused.label ?? "missing",
		"focused_role": focused.role,
		"focused_native_value": jsonValue(focused.nativeValue),
		"key_code": keyCode,
		"flags_raw_value": flags.rawValue,
		"dispatch_elapsed_ms": dispatchElapsed,
	]
}

func captureWindow(
	pid: pid_t,
	screenshotURL: URL
) throws -> [String: Any] {
	guard let windowInfo = CGWindowListCopyWindowInfo(
		[.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID
	) as? [[String: Any]]
	else { throw DiagnosticFailure.message("window server inventory unavailable") }
	let matches = windowInfo.filter {
		($0[kCGWindowOwnerPID as String] as? pid_t) == pid
			&& ($0[kCGWindowName as String] as? String) == expectedWindowTitle
	}
	guard matches.count == 1,
		let windowID = matches[0][kCGWindowNumber as String] as? CGWindowID
	else { throw DiagnosticFailure.message("expected one screenshot window, found \(matches.count)") }
	let capture = Process()
	capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
	capture.arguments = ["-x", "-l", String(windowID), screenshotURL.path]
	try capture.run()
	let deadline = Date().addingTimeInterval(8.0)
	while capture.isRunning, Date() < deadline { Thread.sleep(forTimeInterval: phasePollInterval) }
	if capture.isRunning {
		capture.terminate()
		throw DiagnosticFailure.message("screenshot capture exceeded local timeout")
	}
	guard capture.terminationStatus == 0,
		let image = NSImage(contentsOf: screenshotURL),
		let representation = image.representations.first as? NSBitmapImageRep
	else { throw DiagnosticFailure.message("screenshot capture failed") }
	let width = representation.pixelsWide
	let height = representation.pixelsHigh
	let step = max(1, min(width, height) / 96)
	var opaque = 0
	var colors = Set<UInt32>()
	for y in stride(from: 0, to: height, by: step) {
		for x in stride(from: 0, to: width, by: step) {
			guard let color = representation.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else {
				continue
			}
			if color.alphaComponent > 0.01 { opaque += 1 }
			let red = UInt32((color.redComponent * 255).rounded())
			let green = UInt32((color.greenComponent * 255).rounded())
			let blue = UInt32((color.blueComponent * 255).rounded())
			colors.insert(red << 16 | green << 8 | blue)
		}
	}
	guard width >= 760, height >= 520, opaque > 100, colors.count > 16 else {
		throw DiagnosticFailure.message("screenshot canvas is blank or incorrectly framed")
	}
	return [
		"window_id": windowID,
		"width": width,
		"height": height,
		"sampled_opaque_pixels": opaque,
		"sampled_distinct_colors": colors.count,
		"screenshot_sha256": sha256(screenshotURL) ?? "missing",
	]
}

guard let rawJournalPath = rawArgument("--journal-path") else {
	fputs("missing --journal-path\n", stderr)
	exit(64)
}

let journalURL = normalized(rawJournalPath)
let journal: Journal
do { journal = try Journal(url: journalURL) }
catch { fputs("cannot initialize phase journal: \(error)\n", stderr); exit(70) }

try? journal.append([
	"event": "inspector_started",
	"pid": getpid(),
	"parent_pid": getppid(),
	"executable_path": processPath(getpid()),
	"parent_executable_path": processPath(getppid()),
])

_ = Darwin.setpgid(0, 0)
var phaseResults: [String: Any] = [:]
var expected: Expectations?
var trusted = false
var failure: String?
var recorder: AXRecorder?

do {
	let parsed = try parseExpectations()
	expected = parsed
	let sourceHash = sha256(parsed.sourceURL) ?? "missing"
	let inspectorHash = sha256(parsed.inspectorURL) ?? "missing"
	_ = try phase("startup_tcc", journal: journal, results: &phaseResults) {
		trusted = AXIsProcessTrusted()
		let details: [String: Any] = [
			"process_trusted": trusted,
			"pid": getpid(),
			"parent_pid": getppid(),
			"process_group_id": getpgrp(),
			"executable_path": processPath(getpid()),
			"parent_executable_path": processPath(getppid()),
			"inspector_source_path": parsed.sourceURL.path,
			"inspector_source_sha256": sourceHash,
			"inspector_executable_path": parsed.inspectorURL.path,
			"inspector_executable_sha256": inspectorHash,
			"responsible_process_public_api": "unavailable",
		]
		guard trusted else { throw DiagnosticFailure.message("Accessibility permission denied") }
		guard sourceHash == parsed.sourceSHA256 else {
			throw DiagnosticFailure.message("inspector source hash changed")
		}
		guard inspectorHash == parsed.inspectorSHA256,
			parsed.inspectorURL == normalized(processPath(getpid()))
		else { throw DiagnosticFailure.message("inspector executable identity changed") }
		return ((), details)
	}

	let app = NSRunningApplication(processIdentifier: parsed.pid)
	let actualBundleURL = app?.bundleURL.map { normalized($0.path) }
	let actualExecutableURL = app?.executableURL.map { normalized($0.path) }
	let root = AXUIElementCreateApplication(parsed.pid)
		let ax = AXRecorder(journal: journal)
		recorder = ax
		let window: AXUIElement = try phase(
			"app_window_identity", journal: journal, results: &phaseResults
		) {
		guard let session = CGSessionCopyCurrentDictionary() as? [String: Any],
			let onConsole = session["kCGSSessionOnConsoleKey"] as? Bool
		else {
			throw DiagnosticFailure.message("console session state is unavailable")
		}
		let screenLocked = session["CGSSessionScreenIsLocked"] as? Bool ?? false
		guard onConsole, !screenLocked else {
			throw DiagnosticFailure.message("console session is locked or inactive")
		}
		let timeoutError = AXUIElementSetMessagingTimeout(root, axMessagingTimeout)
		try journal.append([
			"event": "ax_operation",
			"operation": "application.set_messaging_timeout",
			"timeout_seconds": axMessagingTimeout,
			"ax_error": timeoutError.rawValue,
			"elapsed_ms": 0,
		])
		let activationDeadline = Date().addingTimeInterval(4.0)
		var activationAttempts = 0
		repeat {
			activationAttempts += 1
			_ = app?.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
			if app?.isActive == true { break }
			if Date() < activationDeadline {
				Thread.sleep(forTimeInterval: phasePollInterval)
			}
		} while Date() < activationDeadline
		guard app?.isActive == true else {
			throw DiagnosticFailure.message(
				"exact application did not become active after \(activationAttempts) bounded attempts"
			)
		}
		guard app?.isTerminated == false,
			app?.bundleIdentifier == expectedBundleIdentifier,
			actualBundleURL == parsed.bundleURL,
			actualExecutableURL == parsed.executableURL,
			actualExecutableURL.flatMap(sha256) == parsed.executableSHA256
		else {
			throw DiagnosticFailure.message(
				"exact application process identity failed: terminated=\(app?.isTerminated.description ?? "missing") "
					+ "active=\(app?.isActive.description ?? "missing") "
					+ "bundle=\(app?.bundleIdentifier ?? "missing") "
					+ "bundle_path=\(actualBundleURL?.path ?? "missing") "
					+ "executable_path=\(actualExecutableURL?.path ?? "missing") "
					+ "executable_hash=\(actualExecutableURL.flatMap(sha256) ?? "missing")"
			)
		}
		let deadline = Date().addingTimeInterval(4.0)
		var windows: [AXUIElement] = []
		var matches: [AXUIElement] = []
		var lastError = AXError.success
		repeat {
			let (windowsError, windowsValue) = try ax.copy(
				root, kAXWindowsAttribute, operation: "application.windows"
			)
			lastError = windowsError
			windows = windowsValue as? [AXUIElement] ?? []
			matches.removeAll(keepingCapacity: true)
			for (index, candidate) in windows.enumerated() {
				let (_, titleValue) = try ax.copy(
					candidate, kAXTitleAttribute, operation: "window.\(index).title"
				)
				if titleValue as? String == expectedWindowTitle { matches.append(candidate) }
			}
			if windows.count == 1, matches.count == 1 { break }
			Thread.sleep(forTimeInterval: phasePollInterval)
		} while Date() < deadline
		guard windows.count == 1, matches.count == 1, let only = matches.first else {
			throw DiagnosticFailure.message(
				"expected one identified window; last AXError \(lastError.rawValue)"
			)
		}
		return (only, [
			"expected_pid": parsed.pid,
			"actual_pid": app?.processIdentifier ?? -1,
			"bundle_identifier": app?.bundleIdentifier ?? "missing",
			"bundle_url": actualBundleURL?.path ?? "missing",
			"executable_path": actualExecutableURL?.path ?? "missing",
			"executable_sha256": actualExecutableURL.flatMap(sha256) ?? "missing",
				"window_count": windows.count,
				"window_title": expectedWindowTitle,
				"application_active": app?.isActive == true,
				"activation_attempts": activationAttempts,
				"console_session_locked": screenLocked,
				"console_session_on_console": onConsole,
				"ax_messaging_timeout_seconds": axMessagingTimeout,
		])
	}

	let tree = try phase("readonly_tree", journal: journal, results: &phaseResults) {
		let deadline = Date().addingTimeInterval(4.0)
		var tree = try snapshot(window, recorder: ax)
		while !(destinations + ["Open settings"]).allSatisfy({ destination in
			tree.facts.filter { labelMatches($0.label, destination) }.count == 1
		}), Date() < deadline {
			Thread.sleep(forTimeInterval: phasePollInterval)
			tree = try snapshot(window, recorder: ax)
		}
		let destinationFacts = try destinations.map { try fact($0, in: tree) }
		let settingsFact = try fact("Open settings", in: tree)
		let roles = destinationFacts.map(\.role)
		let values = Dictionary(uniqueKeysWithValues: zip(
			destinations,
			destinationFacts.map { jsonValue($0.nativeValue) }
		))
		guard roles.allSatisfy({ $0 == kAXRadioButtonRole }) else {
			throw DiagnosticFailure.message("destination native roles are not AXRadioButton")
		}
		guard destinationFacts.allSatisfy({ $0.nativeValue is Bool }) else {
			throw DiagnosticFailure.message("destination native AXValue is not boolean")
		}
		guard settingsFact.role == kAXButtonRole else {
			throw DiagnosticFailure.message("Settings native role is not AXButton")
		}
		return (tree, [
			"visited_nodes": tree.visited,
			"maximum_nodes": maximumTreeNodes,
			"destination_labels": destinations,
			"destination_roles": roles,
			"destination_native_values": values,
			"settings_label": settingsFact.label ?? "missing",
			"settings_role": settingsFact.role,
			"roles": tree.roles.sorted(),
		])
	}

	_ = try phase("screenshot_pixels", journal: journal, results: &phaseResults) {
		let receipt = try captureWindow(pid: parsed.pid, screenshotURL: parsed.screenshotURL)
		return ((), receipt)
	}

	let baseline = try phase("keyboard_baseline", journal: journal, results: &phaseResults) {
		let focused = try waitForKeyboardBaseline(root: root, recorder: ax)
		let expectedRole = labelMatches(focused.label, destinations[0])
			? kAXRadioButtonRole
			: kAXGroupRole
		guard focused.role == expectedRole else {
			throw DiagnosticFailure.message(
				"keyboard baseline role is \(focused.role), expected \(expectedRole)"
			)
		}
		return (focused, [
			"focused_label": focused.label ?? "missing",
			"focused_role": focused.role,
			"focused_native_value": jsonValue(focused.nativeValue),
		])
	}

	_ = try phase("keyboard_forward", journal: journal, results: &phaseResults) {
		let eventDestinations = baseline.label == expectedShellLabel
			? focusOrder
			: Array(focusOrder.dropFirst())
		var readbacks: [[String: Any]] = []
		for (index, destination) in eventDestinations.enumerated() {
			readbacks.append(try keyboardStep(
				operation: "forward_tab.\(index)",
				expectedLabel: destination,
				keyCode: 48,
				pid: parsed.pid,
				root: root,
				recorder: ax,
				journal: journal
			))
		}
		let observed = (labelMatches(baseline.label, destinations[0]) ? [baseline.label ?? "missing"] : [])
			+ readbacks.compactMap { $0["focused_label"] as? String }
		return ((), [
			"baseline_label": baseline.label ?? "missing",
			"expected_order": focusOrder,
			"observed_order": observed,
			"event_readbacks": readbacks,
		])
	}

	_ = try phase("keyboard_reverse", journal: journal, results: &phaseResults) {
		let settings = try waitForFocused(
			"Open settings", root: root, recorder: ax, operation: "reverse_start.readback"
		)
		var readbacks: [[String: Any]] = []
		for (index, destination) in focusOrder.dropLast().reversed().enumerated() {
			readbacks.append(try keyboardStep(
				operation: "reverse_shift_tab.\(index)",
				expectedLabel: destination,
				keyCode: 48,
				flags: .maskShift,
				pid: parsed.pid,
				root: root,
				recorder: ax,
				journal: journal
			))
		}
		let observed = [settings.label ?? "missing"]
			+ readbacks.compactMap { $0["focused_label"] as? String }
		let expectedOrder = Array(focusOrder.reversed())
		return ((), [
			"expected_order": expectedOrder,
			"observed_order": observed,
			"event_readbacks": readbacks,
		])
	}

	_ = try phase("keyboard_enter_selection", journal: journal, results: &phaseResults) {
		let factory = try fact("Factory", in: tree)
		let quickTasks = try fact("Workbench", in: tree)
		_ = try waitForNativeBool(
			false, element: factory.element, recorder: ax, operation: "enter.factory_before"
		)
		_ = try waitForNativeBool(
			true, element: quickTasks.element, recorder: ax, operation: "enter.conversations_before"
		)
		let interval = 0.04
		let dispatchElapsed = try postKey(36, to: parsed.pid, interval: interval)
		try journal.append([
			"event": "input_operation",
			"operation": "enter_path.enter_factory",
			"key_code": 36,
			"flags_raw_value": 0,
			"down_up_interval_ms": interval * 1_000,
			"elapsed_ms": dispatchElapsed,
		])
		let focusedAfter = try waitForFocused(
			"Factory", root: root, recorder: ax, operation: "enter_path.focus_after"
		)
		let quickTasksSelected = try waitForNativeBool(
			false,
			element: quickTasks.element,
			recorder: ax,
			operation: "enter.conversations_after"
		)
		let factorySelected = try waitForNativeBool(
			true,
			element: factory.element,
			recorder: ax,
			operation: "enter.factory_after"
		)
		return ((), [
			"tab_readbacks": [],
			"focused_label_after_enter": focusedAfter.label ?? "missing",
			"focused_role_after_enter": focusedAfter.role,
			"focused_native_value_after_enter": jsonValue(focusedAfter.nativeValue),
			"conversations_selected_after": quickTasksSelected,
			"factory_selected_after": factorySelected,
			"enter_dispatch_elapsed_ms": dispatchElapsed,
		])
	}
} catch {
	failure = String(describing: error)
}

let report: [String: Any] = [
	"schema": "decodex/gpui-reset-diagnostic-inspector/1",
	"completed_at": ISO8601DateFormatter().string(from: Date()),
	"passed": failure == nil,
	"failure": failure ?? NSNull(),
	"process_trusted": trusted,
	"inspector_pid": getpid(),
	"inspector_parent_pid": getppid(),
	"inspector_process_group_id": getpgrp(),
	"inspector_executable_path": processPath(getpid()),
	"inspector_executable_sha256": expected.flatMap { sha256($0.inspectorURL) } ?? "missing",
	"inspector_source_path": expected?.sourceURL.path ?? "missing",
	"inspector_source_sha256": expected.flatMap { sha256($0.sourceURL) } ?? "missing",
	"responsible_process_public_api": "unavailable",
	"ax_operation_count": recorder?.operationCount ?? 0,
	"phases": phaseResults,
]

if let reportURL = expected?.reportURL ?? rawArgument("--report-path").map(normalized) {
	try? writeJSON(report, to: reportURL)
}
try? journal.append([
	"event": "inspector_completed",
	"passed": failure == nil,
	"failure": failure ?? NSNull(),
])

if failure != nil { exit(trusted ? 2 : 77) }
