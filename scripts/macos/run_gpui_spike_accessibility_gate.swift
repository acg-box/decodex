#!/usr/bin/env swift

import AppKit
import CryptoKit
import Darwin
import Foundation

let bundleIdentifier = "space.decodex.gpui-spike"
let appName = "Decodex GPUI Spike.app"
let executableName = "decodex-gpui-spike"
let launchTimeout = 10.0
let probeTimeout = 45.0
let cleanupTimeout = 5.0
let pollInterval = 0.05

struct Configuration {
	let runs: Int
	let appURL: URL
	let outputURL: URL
	let probeURL: URL
	let harnessURL: URL
}

enum HarnessError: Error, CustomStringConvertible {
	case message(String)

	var description: String {
		switch self {
		case let .message(value): value
		}
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

func normalized(_ url: URL) -> URL {
	url.standardizedFileURL.resolvingSymlinksInPath()
}

func sha256(_ url: URL) throws -> String {
	let data = try Data(contentsOf: url, options: .mappedIfSafe)
	return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

func timestamp() -> String {
	ISO8601DateFormatter().string(from: Date())
}

func writeJSON(_ value: Any, to url: URL) throws {
	guard JSONSerialization.isValidJSONObject(value) else {
		throw HarnessError.message("invalid JSON object for \(url.path)")
	}
	let data = try JSONSerialization.data(
		withJSONObject: value,
		options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
	)
	try data.write(to: url, options: .atomic)
}

func parseConfiguration() throws -> Configuration {
	let scriptURL = normalized(URL(fileURLWithPath: CommandLine.arguments[0]))
	let root = scriptURL.deletingLastPathComponent().deletingLastPathComponent()
		.deletingLastPathComponent()
	var runs = 40
	var appURL = root.appendingPathComponent("target/gpui-spike/\(appName)")
	var outputURL: URL?
	var index = 1

	while index < CommandLine.arguments.count {
		let argument = CommandLine.arguments[index]
		guard index + 1 < CommandLine.arguments.count else {
			throw HarnessError.message("missing value for \(argument)")
		}
		let value = CommandLine.arguments[index + 1]
		switch argument {
		case "--runs":
			guard let parsed = Int(value), (1 ... 100).contains(parsed) else {
				throw HarnessError.message("--runs must be between 1 and 100")
			}
			runs = parsed
		case "--app":
			appURL = URL(fileURLWithPath: value)
		case "--output-dir":
			outputURL = URL(fileURLWithPath: value)
		default:
			throw HarnessError.message("unknown argument: \(argument)")
		}
		index += 2
	}

	let defaultName = "cold-launch-\(Int(Date().timeIntervalSince1970))-\(getpid())"
	return Configuration(
		runs: runs,
		appURL: normalized(appURL),
		outputURL: normalized(outputURL ?? root.appendingPathComponent(
			"target/gpui-spike/evidence/\(defaultName)"
		)),
		probeURL: normalized(root.appendingPathComponent(
			"scripts/macos/inspect_gpui_spike_accessibility.swift"
		)),
		harnessURL: scriptURL
	)
}

func matchingApplications(appURL: URL, executableURL: URL) -> [NSRunningApplication] {
	NSWorkspace.shared.runningApplications.filter { app in
		guard !app.isTerminated else { return false }
		let actualBundle = app.bundleURL.map(normalized)
		let actualExecutable = app.executableURL.map(normalized)
		return app.bundleIdentifier == bundleIdentifier
			|| actualBundle == appURL
			|| actualExecutable == executableURL
	}
}

func waitUntil(timeout: Double, _ predicate: () -> Bool) -> Bool {
	let deadline = Date().addingTimeInterval(timeout)
	repeat {
		if predicate() { return true }
		_ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(pollInterval))
	} while Date() < deadline
	return false
}

func launch(_ appURL: URL) throws -> NSRunningApplication {
	let state = LaunchResult()
	let configuration = NSWorkspace.OpenConfiguration()
	configuration.activates = true
	configuration.createsNewApplicationInstance = true
	configuration.environment = ["DECODEX_GPUI_SPIKE_AUTO_QUIT_MS": "120000"]
	NSWorkspace.shared.openApplication(at: appURL, configuration: configuration) { app, error in
		if let app {
			state.set(.success(app))
		} else {
			state.set(.failure(error ?? HarnessError.message("NSWorkspace returned no application")))
		}
	}
	guard waitUntil(timeout: launchTimeout, { state.get() != nil }), let result = state.get() else {
		throw HarnessError.message("timed out launching staged app")
	}
	return try result.get()
}

func processIdentityMatches(
	_ app: NSRunningApplication,
	appURL: URL,
	executableURL: URL
) -> Bool {
	!app.isTerminated
		&& app.bundleIdentifier == bundleIdentifier
		&& app.bundleURL.map(normalized) == appURL
		&& app.executableURL.map(normalized) == executableURL
}

func cleanup(
	_ app: NSRunningApplication,
	appURL: URL,
	executableURL: URL
) -> [String: Any] {
	let pid = app.processIdentifier
	var method = "already_terminated"
	if !app.isTerminated {
		method = "terminate"
		_ = app.terminate()
	}
	if !waitUntil(timeout: cleanupTimeout, { app.isTerminated })
		&& processIdentityMatches(app, appURL: appURL, executableURL: executableURL)
	{
		method = "sigkill"
		_ = Darwin.kill(pid, SIGKILL)
		_ = waitUntil(timeout: cleanupTimeout, { app.isTerminated })
	}
	return [
		"method": method,
		"pid": pid,
		"terminated": app.isTerminated,
		"remaining_matching_pids": matchingApplications(
			appURL: appURL,
			executableURL: executableURL
		).map(\.processIdentifier).sorted(),
	]
}

func runProbe(
	configuration: Configuration,
	pid: pid_t,
	executableURL: URL,
	executableSHA256: String,
	probeSHA256: String,
	runURL: URL
) throws -> [String: Any] {
	let outputURL = runURL.appendingPathComponent("probe.json")
	let errorURL = runURL.appendingPathComponent("probe.stderr")
	_ = FileManager.default.createFile(atPath: outputURL.path, contents: nil)
	_ = FileManager.default.createFile(atPath: errorURL.path, contents: nil)
	let outputHandle = try FileHandle(forWritingTo: outputURL)
	let errorHandle = try FileHandle(forWritingTo: errorURL)
	defer {
		try? outputHandle.close()
		try? errorHandle.close()
	}
	let process = Process()
	process.executableURL = URL(fileURLWithPath: "/usr/bin/swift")
	process.arguments = [
		configuration.probeURL.path,
		"--expected-pid", String(pid),
		"--expected-bundle-url", configuration.appURL.path,
		"--expected-executable-path", executableURL.path,
		"--expected-executable-sha256", executableSHA256,
		"--probe-path", configuration.probeURL.path,
		"--expected-probe-sha256", probeSHA256,
	]
	process.standardOutput = outputHandle
	process.standardError = errorHandle
	try process.run()
	let completed = waitUntil(timeout: probeTimeout) { !process.isRunning }
	if !completed {
		process.terminate()
		_ = waitUntil(timeout: 2.0) { !process.isRunning }
		if process.isRunning {
			_ = Darwin.kill(process.processIdentifier, SIGKILL)
			_ = waitUntil(timeout: 2.0) { !process.isRunning }
		}
	}
	try outputHandle.synchronize()
	try errorHandle.synchronize()
	let status = completed ? Int(process.terminationStatus) : 124
	try "\(status)\n".write(
		to: runURL.appendingPathComponent("probe.exit-status"),
		atomically: true,
		encoding: .utf8
	)
	let output = try Data(contentsOf: outputURL)
	let reportValid = (try? JSONSerialization.jsonObject(with: output)) is [String: Any]
	return [
		"completed_before_timeout": completed,
		"exit_status": status,
		"report_valid": reportValid,
		"passed": completed && status == 0 && reportValid,
	]
}

func runOne(
	index: Int,
	configuration: Configuration,
	executableURL: URL,
	executableSHA256: String,
	probeSHA256: String,
	harnessSHA256: String
) throws -> [String: Any] {
	let runURL = configuration.outputURL.appendingPathComponent(
		String(format: "run-%03d", index)
	)
	try FileManager.default.createDirectory(at: runURL, withIntermediateDirectories: false)
	let preexisting = matchingApplications(
		appURL: configuration.appURL,
		executableURL: executableURL
	)
	guard preexisting.isEmpty else {
		throw HarnessError.message(
			"run \(index) refused preexisting matching PIDs: \(preexisting.map(\.processIdentifier))"
		)
	}

	let startedAt = timestamp()
	let app = try launch(configuration.appURL)
	let launchedPID = app.processIdentifier
	var matchingPIDs: [pid_t] = []
	var pidUniquenessAttempts = 0
	let pidUnique = waitUntil(timeout: launchTimeout) {
		pidUniquenessAttempts += 1
		matchingPIDs = matchingApplications(
			appURL: configuration.appURL,
			executableURL: executableURL
		).map(\.processIdentifier).sorted()
		return matchingPIDs == [launchedPID]
	}
	let launchIdentityValid = pidUnique && processIdentityMatches(
		app,
		appURL: configuration.appURL,
		executableURL: executableURL
	)
	var launcherActivationAttempts = 0
	let launcherActivationValid = waitUntil(timeout: launchTimeout) {
		launcherActivationAttempts += 1
		return app.isActive
	}
	var harnessError: String?
	var probe: [String: Any] = [
		"completed_before_timeout": false,
		"exit_status": -1,
		"report_valid": false,
		"passed": false,
	]
	if !pidUnique || !launchIdentityValid || !launcherActivationValid {
		harnessError = "launch failed: launched \(launchedPID), matching \(matchingPIDs), active \(launcherActivationValid)"
	} else {
		do {
			probe = try runProbe(
				configuration: configuration,
				pid: launchedPID,
				executableURL: executableURL,
				executableSHA256: executableSHA256,
				probeSHA256: probeSHA256,
				runURL: runURL
			)
		} catch {
			harnessError = "probe failed: \(error)"
		}
	}
	let cleanupResult = cleanup(
		app,
		appURL: configuration.appURL,
		executableURL: executableURL
	)
	let cleanupValid = cleanupResult["terminated"] as? Bool == true
		&& (cleanupResult["remaining_matching_pids"] as? [pid_t])?.isEmpty == true
	if !cleanupValid, harnessError == nil {
		harnessError = "cleanup failed"
	}
	let summary: [String: Any] = [
		"run": index,
		"started_at": startedAt,
		"completed_at": timestamp(),
		"expected_pid": launchedPID,
		"launched_pid": launchedPID,
		"matching_pids_after_launch": matchingPIDs,
		"pid_unique": pidUnique,
		"pid_uniqueness_attempts": pidUniquenessAttempts,
		"bundle_identifier": bundleIdentifier,
		"bundle_url": configuration.appURL.path,
		"executable_path": executableURL.path,
		"executable_sha256": executableSHA256,
		"probe_path": configuration.probeURL.path,
		"probe_sha256": probeSHA256,
		"harness_path": configuration.harnessURL.path,
		"harness_sha256": harnessSHA256,
		"launch_identity_valid": launchIdentityValid,
		"launcher_activation_attempts": launcherActivationAttempts,
		"launcher_activation_valid": launcherActivationValid,
		"probe": probe,
		"cleanup": cleanupResult,
		"harness_error": harnessError ?? NSNull(),
		"passed": harnessError == nil
			&& pidUnique
			&& launchIdentityValid
			&& launcherActivationValid
			&& (probe["passed"] as? Bool == true)
			&& cleanupValid,
	]
	try writeJSON(summary, to: runURL.appendingPathComponent("summary.json"))
	return summary
}

func main() throws {
	let configuration = try parseConfiguration()
	let executableURL = configuration.appURL.appendingPathComponent(
		"Contents/MacOS/\(executableName)"
	)
	guard FileManager.default.fileExists(atPath: configuration.appURL.path),
		FileManager.default.isExecutableFile(atPath: executableURL.path),
		FileManager.default.fileExists(atPath: configuration.probeURL.path),
		let stagedBundle = Bundle(url: configuration.appURL),
		stagedBundle.bundleIdentifier == bundleIdentifier,
		stagedBundle.executableURL.map(normalized) == executableURL
	else {
		throw HarnessError.message("staged app, executable, bundle identity, or probe is invalid")
	}
	guard !FileManager.default.fileExists(atPath: configuration.outputURL.path) else {
		throw HarnessError.message("output directory already exists: \(configuration.outputURL.path)")
	}
	let preexisting = matchingApplications(
		appURL: configuration.appURL,
		executableURL: executableURL
	)
	guard preexisting.isEmpty else {
		throw HarnessError.message(
			"refusing preexisting matching PIDs: \(preexisting.map(\.processIdentifier).sorted())"
		)
	}

	try FileManager.default.createDirectory(
		at: configuration.outputURL,
		withIntermediateDirectories: true
	)
	let executableSHA256 = try sha256(executableURL)
	let probeSHA256 = try sha256(configuration.probeURL)
	let harnessSHA256 = try sha256(configuration.harnessURL)
	var runs: [[String: Any]] = []
	let startedAt = timestamp()

	for index in 1 ... configuration.runs {
		do {
			runs.append(try runOne(
				index: index,
				configuration: configuration,
					executableURL: executableURL,
					executableSHA256: executableSHA256,
					probeSHA256: probeSHA256,
					harnessSHA256: harnessSHA256
			))
		} catch {
			let failure: [String: Any] = [
				"run": index,
				"passed": false,
				"harness_error": String(describing: error),
				"bundle_url": configuration.appURL.path,
				"executable_path": executableURL.path,
				"executable_sha256": executableSHA256,
					"probe_path": configuration.probeURL.path,
					"probe_sha256": probeSHA256,
					"harness_path": configuration.harnessURL.path,
					"harness_sha256": harnessSHA256,
			]
			runs.append(failure)
			let runURL = configuration.outputURL.appendingPathComponent(
				String(format: "run-%03d", index)
			)
			if !FileManager.default.fileExists(atPath: runURL.path) {
				try FileManager.default.createDirectory(
					at: runURL,
					withIntermediateDirectories: false
				)
			}
			try writeJSON(failure, to: runURL.appendingPathComponent("summary.json"))
			break
		}
		let partial: [String: Any] = [
			"schema": "decodex/gpui-accessibility-cold-launch/1",
			"started_at": startedAt,
			"runs_requested": configuration.runs,
			"runs_completed": runs.count,
			"harness_path": configuration.harnessURL.path,
			"harness_sha256": harnessSHA256,
			"runs": runs,
		]
		try writeJSON(partial, to: configuration.outputURL.appendingPathComponent("summary.json"))
	}

	let passed = runs.filter { $0["passed"] as? Bool == true }.count
	let summary: [String: Any] = [
		"schema": "decodex/gpui-accessibility-cold-launch/1",
		"started_at": startedAt,
		"completed_at": timestamp(),
		"runs_requested": configuration.runs,
		"runs_completed": runs.count,
		"runs_passed": passed,
		"overall_acceptance": runs.count == configuration.runs && passed == configuration.runs,
		"bundle_identifier": bundleIdentifier,
		"bundle_url": configuration.appURL.path,
		"executable_path": executableURL.path,
		"executable_sha256": executableSHA256,
		"probe_path": configuration.probeURL.path,
		"probe_sha256": probeSHA256,
		"harness_path": configuration.harnessURL.path,
		"harness_sha256": harnessSHA256,
		"runs": runs,
	]
	try writeJSON(summary, to: configuration.outputURL.appendingPathComponent("summary.json"))
	print(configuration.outputURL.path)
	guard runs.count == configuration.runs, passed == configuration.runs else {
		throw HarnessError.message("cold-launch gate failed: \(passed)/\(configuration.runs) passed")
	}
}

do {
	try main()
} catch {
	fputs("gpui accessibility gate: \(error)\n", stderr)
	exit(1)
}
