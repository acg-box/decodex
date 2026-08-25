#!/usr/bin/env swift

import AppKit
import CryptoKit
import Darwin
import Foundation

let bundleIdentifier = "box.acg.decodex"
let executableName = "decodex-gpui"
let launchTimeout = 12.0
let inspectorSafetyTimeout = 60.0
let cleanupTimeout = 5.0
let pollInterval = 0.05

struct Configuration {
	let appURL: URL
	let outputURL: URL
	let inspectorSourceURL: URL
	let harnessURL: URL
	let rootURL: URL
}

struct CommandResult {
	let status: Int32
	let stdout: String
	let stderr: String
	let timedOut: Bool
}

enum GateError: Error, CustomStringConvertible {
	case message(String)

	var description: String {
		if case let .message(value) = self { value } else { "unknown reset diagnostic failure" }
	}
}

final class LaunchResult: @unchecked Sendable {
	private let lock = NSLock()
	private var value: Result<NSRunningApplication, Error>?

	func set(_ result: Result<NSRunningApplication, Error>) {
		lock.lock()
		value = result
		lock.unlock()
	}

	func get() -> Result<NSRunningApplication, Error>? {
		lock.lock()
		defer { lock.unlock() }
		return value
	}
}

final class InterruptState: @unchecked Sendable {
	private let lock = NSLock()
	private var value: Int32?

	func set(_ signal: Int32) {
		lock.lock()
		if value == nil { value = signal }
		lock.unlock()
	}

	func get() -> Int32? {
		lock.lock()
		defer { lock.unlock() }
		return value
	}
}

func normalized(_ url: URL) -> URL {
	url.standardizedFileURL.resolvingSymlinksInPath()
}

func sha256(_ url: URL) throws -> String {
	let data = try Data(contentsOf: url, options: .mappedIfSafe)
	return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

func bundleFingerprint(_ bundleURL: URL) throws -> String {
	guard let enumerator = FileManager.default.enumerator(
		at: bundleURL,
		includingPropertiesForKeys: [.isRegularFileKey],
		options: [.skipsHiddenFiles]
	) else { throw GateError.message("cannot enumerate bundle") }
	let files = enumerator.compactMap { $0 as? URL }.filter {
		(try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
	}.sorted { $0.path < $1.path }
	var digest = SHA256()
	for file in files {
		let relative = String(file.path.dropFirst(bundleURL.path.count))
		digest.update(data: Data(relative.utf8))
		digest.update(data: try Data(contentsOf: file, options: .mappedIfSafe))
	}
	return digest.finalize().map { String(format: "%02x", $0) }.joined()
}

func timestamp() -> String {
	ISO8601DateFormatter().string(from: Date())
}

func writeJSON(_ value: Any, to url: URL) throws {
	guard JSONSerialization.isValidJSONObject(value) else {
		throw GateError.message("invalid JSON receipt")
	}
	let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
	try data.write(to: url, options: .atomic)
	let handle = try FileHandle(forWritingTo: url)
	defer { try? handle.close() }
	try handle.synchronize()
	_ = Darwin.fsync(handle.fileDescriptor)
}

func processPath(_ pid: pid_t) -> String {
	var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN) * 4)
	let length = proc_pidpath(pid, &buffer, UInt32(buffer.count))
	guard length > 0 else { return "unavailable" }
	return String(cString: buffer)
}

func parseConfiguration() throws -> Configuration {
	let script = normalized(URL(fileURLWithPath: CommandLine.arguments[0]))
	let root = script.deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
	var app = root.appendingPathComponent("target/decodex-app/Decodex.app")
	var output: URL?
	var index = 1
	while index < CommandLine.arguments.count {
		guard index + 1 < CommandLine.arguments.count else {
			throw GateError.message("missing value for \(CommandLine.arguments[index])")
		}
		switch CommandLine.arguments[index] {
		case "--app": app = URL(fileURLWithPath: CommandLine.arguments[index + 1])
		case "--output-dir": output = URL(fileURLWithPath: CommandLine.arguments[index + 1])
		default: throw GateError.message("unknown argument \(CommandLine.arguments[index])")
		}
		index += 2
	}
	guard let output else { throw GateError.message("--output-dir is required") }
	return Configuration(
		appURL: normalized(app),
		outputURL: normalized(output),
		inspectorSourceURL: normalized(
			root.appendingPathComponent("scripts/macos/inspect_decodex_gpui_accessibility.swift")
		),
		harnessURL: script,
		rootURL: root
	)
}

func waitUntil(timeout: Double, interrupted: InterruptState? = nil, _ predicate: () -> Bool) -> Bool {
	let deadline = Date().addingTimeInterval(timeout)
	repeat {
		if predicate() { return true }
		if interrupted?.get() != nil { return false }
		_ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(pollInterval))
	} while Date() < deadline
	return false
}

func runCommand(_ executable: URL, _ arguments: [String], timeout: Double) throws -> CommandResult {
	let output = Pipe()
	let error = Pipe()
	let process = Process()
	process.executableURL = executable
	process.arguments = arguments
	process.standardOutput = output
	process.standardError = error
	try process.run()
	let completed = waitUntil(timeout: timeout) { !process.isRunning }
	if !completed {
		process.terminate()
		_ = waitUntil(timeout: 2.0) { !process.isRunning }
		if process.isRunning { _ = Darwin.kill(process.processIdentifier, SIGKILL) }
	}
	let stdout = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
	let stderr = String(decoding: error.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
	return CommandResult(
		status: completed ? process.terminationStatus : 124,
		stdout: stdout,
		stderr: stderr,
		timedOut: !completed
	)
}

func matchingApplications(appURL: URL, executableURL: URL) -> [NSRunningApplication] {
	NSWorkspace.shared.runningApplications.filter {
		!$0.isTerminated && ($0.bundleIdentifier == bundleIdentifier
			|| $0.bundleURL.map(normalized) == appURL
			|| $0.executableURL.map(normalized) == executableURL)
	}
}

func launch(_ appURL: URL) throws -> NSRunningApplication {
	let result = LaunchResult()
	let options = NSWorkspace.OpenConfiguration()
	options.activates = true
	options.createsNewApplicationInstance = true
	NSWorkspace.shared.openApplication(at: appURL, configuration: options) { app, error in
		if let app { result.set(.success(app)) }
		else { result.set(.failure(error ?? GateError.message("launcher returned no app"))) }
	}
	guard waitUntil(timeout: launchTimeout, { result.get() != nil }), let value = result.get() else {
		throw GateError.message("launch timed out")
	}
	return try value.get()
}

func cleanupApp(
	_ app: NSRunningApplication?,
	appURL: URL,
	executableURL: URL
) -> [String: Any] {
	guard let app else {
		return [
			"method": "not_launched",
			"terminated": true,
			"remaining_matching_pids": matchingApplications(
				appURL: appURL, executableURL: executableURL
			).map(\.processIdentifier).sorted(),
		]
	}
	let pid = app.processIdentifier
	var method = "already_terminated"
	if !app.isTerminated {
		method = "terminate"
		_ = app.terminate()
	}
	if !waitUntil(timeout: cleanupTimeout, { app.isTerminated }),
		app.bundleURL.map(normalized) == appURL,
		app.executableURL.map(normalized) == executableURL
	{
		method = "sigkill"
		_ = Darwin.kill(pid, SIGKILL)
		_ = waitUntil(timeout: cleanupTimeout) { app.isTerminated }
	}
	return [
		"pid": pid,
		"method": method,
		"terminated": app.isTerminated,
		"remaining_matching_pids": matchingApplications(
			appURL: appURL, executableURL: executableURL
		).map(\.processIdentifier).sorted(),
	]
}

func terminateInspectorGroup(_ process: Process?) -> [String: Any] {
	guard let process else { return ["method": "not_launched", "terminated": true] }
	let pid = process.processIdentifier
	let group = getpgid(pid)
	var method = "already_terminated"
	if process.isRunning {
		method = group == pid ? "process_group_sigterm" : "process_sigterm"
		_ = Darwin.kill(group == pid ? -pid : pid, SIGTERM)
	}
	if !waitUntil(timeout: 2.0, { !process.isRunning }) {
		method = group == pid ? "process_group_sigkill" : "process_sigkill"
		_ = Darwin.kill(group == pid ? -pid : pid, SIGKILL)
		_ = waitUntil(timeout: 2.0) { !process.isRunning }
	}
	return [
		"pid": pid,
		"process_group_id": group,
		"method": method,
		"terminated": !process.isRunning,
	]
}

func installSignalHandling(_ state: InterruptState) -> [DispatchSourceSignal] {
	var sources: [DispatchSourceSignal] = []
	for number in [SIGINT, SIGTERM] {
		Darwin.signal(number, SIG_IGN)
		let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
		source.setEventHandler { state.set(number) }
		source.resume()
		sources.append(source)
	}
	return sources
}

func main() throws -> Bool {
	let configuration = try parseConfiguration()
	guard !FileManager.default.fileExists(atPath: configuration.outputURL.path) else {
		throw GateError.message("output directory already exists: \(configuration.outputURL.path)")
	}
	try FileManager.default.createDirectory(at: configuration.outputURL, withIntermediateDirectories: true)

	let summaryURL = configuration.outputURL.appendingPathComponent("summary.json")
	let compileReceiptURL = configuration.outputURL.appendingPathComponent("compile-receipt.json")
	let executableURL = configuration.appURL.appendingPathComponent("Contents/MacOS/\(executableName)")
	let inspectorURL = configuration.outputURL.appendingPathComponent("decodex-gpui-ax-inspector")
	let journalURL = configuration.outputURL.appendingPathComponent("phase-journal.jsonl")
	let inspectorReportURL = configuration.outputURL.appendingPathComponent("inspector-report.json")
	let screenshotURL = configuration.outputURL.appendingPathComponent("window.png")
	let inspectorStdoutURL = configuration.outputURL.appendingPathComponent("inspector.stdout")
	let inspectorStderrURL = configuration.outputURL.appendingPathComponent("inspector.stderr")
	let interruptState = InterruptState()
	let signalSources = installSignalHandling(interruptState)
	defer { signalSources.forEach { $0.cancel() } }

	var failure: String?
	var compileReceipt: [String: Any] = [:]
	var inspectorReceipt: [String: Any] = [:]
	var app: NSRunningApplication?
	var inspector: Process?
	var launchIdentityValid = false
	var bundleHashBefore = "missing"
	var executableHash = "missing"
	var sourceHash = "missing"
	var inspectorHash = "missing"
	var matchingPIDs: [pid_t] = []

	do {
		guard let bundle = Bundle(url: configuration.appURL),
			bundle.bundleIdentifier == bundleIdentifier,
			bundle.executableURL.map(normalized) == executableURL,
			FileManager.default.isExecutableFile(atPath: executableURL.path),
			FileManager.default.fileExists(atPath: configuration.inspectorSourceURL.path)
		else { throw GateError.message("frozen staged app or inspector source is invalid") }
		guard matchingApplications(appURL: configuration.appURL, executableURL: executableURL).isEmpty else {
			throw GateError.message("matching candidate application is already running")
		}

		bundleHashBefore = try bundleFingerprint(configuration.appURL)
		executableHash = try sha256(executableURL)
		sourceHash = try sha256(configuration.inspectorSourceURL)
		let harnessHash = try sha256(configuration.harnessURL)
		let compilerLookup = try runCommand(
			URL(fileURLWithPath: "/usr/bin/xcrun"), ["--find", "swiftc"], timeout: 10.0
		)
		guard compilerLookup.status == 0 else {
			throw GateError.message("cannot resolve Swift compiler: \(compilerLookup.stderr)")
		}
		let compilerPath = compilerLookup.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
		let compilerURL = URL(fileURLWithPath: compilerPath)
		let sdkLookup = try runCommand(
			URL(fileURLWithPath: "/usr/bin/xcrun"),
			["--sdk", "macosx", "--show-sdk-path"],
			timeout: 10.0
		)
		guard sdkLookup.status == 0 else {
			throw GateError.message("cannot resolve macOS SDK: \(sdkLookup.stderr)")
		}
		let sdkPath = sdkLookup.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
		let compilerVersion = try runCommand(compilerURL, ["--version"], timeout: 10.0)
		guard compilerVersion.status == 0 else {
			throw GateError.message("cannot read Swift compiler version")
		}
		let compile = try runCommand(
			compilerURL,
			["-sdk", sdkPath, configuration.inspectorSourceURL.path, "-o", inspectorURL.path],
			timeout: 60.0
		)
		try compile.stdout.write(
			to: configuration.outputURL.appendingPathComponent("compiler.stdout"),
			atomically: true,
			encoding: .utf8
		)
		try compile.stderr.write(
			to: configuration.outputURL.appendingPathComponent("compiler.stderr"),
			atomically: true,
			encoding: .utf8
		)
		guard compile.status == 0, FileManager.default.isExecutableFile(atPath: inspectorURL.path) else {
			throw GateError.message("ahead-of-time inspector compilation failed")
		}
		let signing = try runCommand(
			URL(fileURLWithPath: "/usr/bin/codesign"),
			["--force", "--timestamp=none", "--sign", "-", inspectorURL.path],
			timeout: 10.0
		)
		guard signing.status == 0 else {
			throw GateError.message("inspector ad hoc signing failed: \(signing.stderr)")
		}
		let signingIdentity = try runCommand(
			URL(fileURLWithPath: "/usr/bin/codesign"),
			["-dvv", inspectorURL.path],
			timeout: 10.0
		)
		guard signingIdentity.status == 0 else {
			throw GateError.message("cannot inspect generated inspector signature")
		}
		inspectorHash = try sha256(inspectorURL)
		compileReceipt = [
			"schema": "decodex/gpui-reset-diagnostic-compile/1",
			"completed_at": timestamp(),
			"compiler_path": compilerURL.path,
			"compiler_version": compilerVersion.stdout.trimmingCharacters(in: .whitespacesAndNewlines),
			"compiler_stderr": compilerVersion.stderr,
			"compiler_sdk_path": sdkPath,
			"inspector_source_path": configuration.inspectorSourceURL.path,
			"inspector_source_sha256": sourceHash,
			"inspector_executable_path": inspectorURL.path,
			"inspector_executable_sha256": inspectorHash,
			"inspector_codesign_details": signingIdentity.stderr,
			"inspector_codesign_identity": "adhoc",
			"harness_path": configuration.harnessURL.path,
			"harness_sha256": harnessHash,
			"harness_pid": getpid(),
			"harness_parent_pid": getppid(),
			"harness_process_path": processPath(getpid()),
			"harness_parent_process_path": processPath(getppid()),
			"responsible_process_public_api": "unavailable",
		]
		try writeJSON(compileReceipt, to: compileReceiptURL)

		app = try launch(configuration.appURL)
		guard let app else { throw GateError.message("launcher returned no application") }
		let launchedPID = app.processIdentifier
		launchIdentityValid = waitUntil(timeout: launchTimeout, interrupted: interruptState) {
			matchingPIDs = matchingApplications(
				appURL: configuration.appURL, executableURL: executableURL
			).map(\.processIdentifier).sorted()
			return matchingPIDs == [launchedPID]
				&& app.bundleIdentifier == bundleIdentifier
				&& app.bundleURL.map(normalized) == configuration.appURL
				&& app.executableURL.map(normalized) == executableURL
		}
		guard launchIdentityValid else { throw GateError.message("exact app launch identity failed") }
		_ = app.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])

		FileManager.default.createFile(atPath: inspectorStdoutURL.path, contents: nil)
		FileManager.default.createFile(atPath: inspectorStderrURL.path, contents: nil)
		let inspectorStdout = try FileHandle(forWritingTo: inspectorStdoutURL)
		let inspectorStderr = try FileHandle(forWritingTo: inspectorStderrURL)
		defer { try? inspectorStdout.close(); try? inspectorStderr.close() }
		let process = Process()
		process.executableURL = inspectorURL
		process.arguments = [
			"--expected-pid", String(launchedPID),
			"--expected-bundle-url", configuration.appURL.path,
			"--expected-executable-path", executableURL.path,
			"--expected-executable-sha256", executableHash,
			"--inspector-source-path", configuration.inspectorSourceURL.path,
			"--expected-inspector-source-sha256", sourceHash,
			"--inspector-executable-path", inspectorURL.path,
			"--expected-inspector-executable-sha256", inspectorHash,
			"--journal-path", journalURL.path,
			"--report-path", inspectorReportURL.path,
			"--screenshot-path", screenshotURL.path,
		]
		process.standardOutput = inspectorStdout
		process.standardError = inspectorStderr
		inspector = process
		try process.run()
		let inspectorPID = process.processIdentifier
		let processGroupReady = waitUntil(timeout: 1.0, interrupted: interruptState) {
			getpgid(inspectorPID) == inspectorPID || !process.isRunning
		}
		guard processGroupReady, getpgid(inspectorPID) == inspectorPID else {
			throw GateError.message("inspector did not establish its process group")
		}
		let completed = waitUntil(
			timeout: inspectorSafetyTimeout,
			interrupted: interruptState
		) { !process.isRunning }
		if !completed {
			let reason = interruptState.get().map { "interrupted by signal \($0)" }
				?? "inspector exceeded final safety ceiling"
			inspectorReceipt["safety_failure"] = reason
			throw GateError.message(reason)
		}
		try inspectorStdout.synchronize()
		try inspectorStderr.synchronize()
		let status = process.terminationStatus
		let reportData = try? Data(contentsOf: inspectorReportURL)
		let report = reportData.flatMap {
			try? JSONSerialization.jsonObject(with: $0) as? [String: Any]
		}
		inspectorReceipt.merge([
			"inspector_pid": inspectorPID,
			"inspector_parent_pid": getpid(),
			"inspector_process_group_id": inspectorPID,
			"inspector_executable_path": inspectorURL.path,
			"inspector_executable_sha256": inspectorHash,
			"inspector_source_path": configuration.inspectorSourceURL.path,
			"inspector_source_sha256": sourceHash,
			"exit_status": status,
			"report_valid": report != nil,
			"phase_journal_path": journalURL.path,
			"phase_journal_sha256": (try? sha256(journalURL)) ?? "missing",
			"passed": status == 0 && report?["passed"] as? Bool == true,
		]) { _, new in new }
		guard inspectorReceipt["passed"] as? Bool == true else {
			throw GateError.message("compiled inspector reported a diagnostic phase failure")
		}
	} catch {
		failure = String(describing: error)
	}

	let inspectorCleanup = terminateInspectorGroup(inspector)
	let appCleanup = cleanupApp(app, appURL: configuration.appURL, executableURL: executableURL)
	let bundleHashAfter = (try? bundleFingerprint(configuration.appURL)) ?? "missing"
	let cleanupValid = inspectorCleanup["terminated"] as? Bool == true
		&& appCleanup["terminated"] as? Bool == true
		&& (appCleanup["remaining_matching_pids"] as? [pid_t])?.isEmpty == true
	let passed = failure == nil
		&& inspectorReceipt["passed"] as? Bool == true
		&& launchIdentityValid
		&& cleanupValid
		&& bundleHashBefore == bundleHashAfter
	let summary: [String: Any] = [
		"schema": "decodex/gpui-reset-diagnostic/1",
		"completed_at": timestamp(),
		"passed": passed,
		"failure": failure ?? NSNull(),
		"bundle_identifier": bundleIdentifier,
		"bundle_url": configuration.appURL.path,
		"bundle_fingerprint_before": bundleHashBefore,
		"bundle_fingerprint_after": bundleHashAfter,
		"executable_path": executableURL.path,
		"executable_sha256": executableHash,
		"launch_identity_valid": launchIdentityValid,
		"matching_pids_after_launch": matchingPIDs,
		"compile": compileReceipt,
		"inspector": inspectorReceipt,
		"inspector_cleanup": inspectorCleanup,
		"app_cleanup": appCleanup,
		"interrupted_signal": interruptState.get() ?? NSNull(),
	]
	try writeJSON(summary, to: summaryURL)
	print(configuration.outputURL.path)
	return passed
}

do {
	guard try main() else { exit(1) }
} catch {
	fputs("decodex GPUI reset diagnostic: \(error)\n", stderr)
	exit(1)
}
