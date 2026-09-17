import AppKit
import Darwin
import XCTest

@testable import DecodexApp

@MainActor
final class VoiceMediaHostTests: XCTestCase {
    func testNativeWebRTCProducesOfferAndStopsWithoutMicrophoneAccess() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "Decodex Voice Test"
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let sdp = try XCTUnwrap(offer["sdp"] as? String)
        XCTAssertTrue(sdp.contains("m=audio"))
        XCTAssertTrue(sdp.contains("m=application"))
        let diagnostics = await host.connectionDiagnostics()
        let state = try JSONSerialization.jsonObject(with: Data(diagnostics.utf8)) as? [String: Any]
        XCTAssertEqual(state?["muted"] as? Bool, true, "Mute requested before capture must apply to the first track")
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        host.close()
        XCTAssertFalse(host.command(#"{"operation":"start"}"#))
    }


    func testOptInNativeSubscriptionCall() async throws {
        let previousSignal = signal(SIGPIPE, SIG_IGN)
        defer { signal(SIGPIPE, previousSignal) }
        let environment = ProcessInfo.processInfo.environment
        guard let helper = environment["DECODEX_VOICE_SIGNAL_HELPER"],
              let root = environment["DECODEX_VOICE_SERVICE_ROOT"],
              let work = environment["DECODEX_VOICE_WORK_ID"],
              let sample = environment["DECODEX_VOICE_SAMPLE"] else {
            throw XCTSkip("Explicit voice qualification profile and synthetic audio required")
        }
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.regular)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "Decodex Voice Test"
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        defer { window.orderOut(nil) }
        let host = VoiceMediaHost(syntheticAudioForTesting: true, sampleForTesting: try Data(contentsOf: URL(fileURLWithPath: sample)), hostWindow: window)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let description = try XCTUnwrap(offer["sdp"] as? String)
        let process = Process(), input = Pipe(), output = Pipe()
        process.executableURL = URL(fileURLWithPath: helper)
        process.arguments = [root, work]
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.standardError
        try process.run()
        defer {
            try? input.fileHandleForWriting.write(contentsOf: Data("stop\n".utf8))
            try? input.fileHandleForWriting.close()
            if process.isRunning { process.terminate() }
        }
        var encoded = try JSONSerialization.data(withJSONObject: description, options: .fragmentsAllowed)
        encoded.append(10)
        try input.fileHandleForWriting.write(contentsOf: encoded)
        let readHandle = output.fileHandleForReading
        let response = await Task.detached {
            var bytes = Data()
            while bytes.count <= 70_000 {
                let chunk = readHandle.availableData
                if chunk.isEmpty { break }
                bytes.append(chunk)
                if bytes.contains(10) { break }
            }
            return bytes
        }.value
        let answer = try XCTUnwrap(JSONSerialization.jsonObject(with: response, options: .fragmentsAllowed) as? String)
        let command = try JSONSerialization.data(withJSONObject: ["operation":"answer", "sdp":answer])
        XCTAssertTrue(host.command(String(decoding: command, as: UTF8.self)))
        _ = try await event(from: host, type: "connected")
        let deadline = Date().addingTimeInterval(55)
        var heardUser = false, heardAssistant = false, finalizedUser = false
        while Date() < deadline && !(heardUser && heardAssistant && finalizedUser) {
            if let pointer = host.poll() {
                let value = try JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any]
                if value?["type"] as? String == "error" { XCTFail("Native voice failed after connection"); break }
                let caption = value?["event"] as? [String: Any]
                heardUser = heardUser || caption?["type"] as? String == "input_transcript.added"
                heardAssistant = heardAssistant || caption?["type"] as? String == "output_transcript.added"
                let turn = caption?["turn"] as? [String: Any]
                finalizedUser = finalizedUser || (caption?["type"] as? String == "turn.done" && turn?["role"] as? String == "user")
            }
            try await Task.sleep(for: .milliseconds(20))
        }
        let diagnostics = await host.connectionDiagnostics()
        XCTAssertTrue(heardUser, "No live input transcript from synthetic speech: \(diagnostics)")
        XCTAssertTrue(heardAssistant, "No live assistant reply")
        XCTAssertTrue(finalizedUser, "No final user transcript")
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        try input.fileHandleForWriting.write(contentsOf: Data("stop\n".utf8))
        for _ in 0..<100 where process.isRunning { try await Task.sleep(for: .milliseconds(20)) }
        XCTAssertFalse(process.isRunning, "Signaling helper did not close")
    }

    private func event(from host: VoiceMediaHost, type: String) async throws -> [String: Any] {
        let deadline = Date().addingTimeInterval(32)
        while Date() < deadline {
            if let pointer = host.poll() {
                let data = Data(String(cString: pointer).utf8)
                let event = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
                if event["type"] as? String == type { return event }
                if event["type"] as? String == "error" {
                    XCTFail(event["message"] as? String ?? "Native audio failed")
                    throw NSError(domain: "VoiceMediaHostTests", code: 1)
                }
            }
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTFail("No native media event: \(type); \(await host.connectionDiagnostics())")
        throw NSError(domain: "VoiceMediaHostTests", code: 2)
    }
}
