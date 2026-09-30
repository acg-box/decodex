import AppKit
import XCTest

@testable import DecodexApp

@MainActor
final class VoiceMediaHostTests: XCTestCase {
    func testNativeDictationFinishFlushesButSupersededCallbacksAreIgnored() async throws {
        var captures: [DictationProbe] = []
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(authorizationRequestForTesting: { authorizations.append($0) },
                                  dictationFactoryForTesting: { emit in
            let capture = DictationProbe(emit: emit)
            captures.append(capture)
            return capture
        })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        authorizations[0](true)
        XCTAssertEqual(captures.count, 1)
        XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
        XCTAssertEqual(captures[0].finishes, 1)
        captures[0].emit(["type":"pcm", "audio":"final"])
        captures[0].emit(["type":"ended"])
        let finalPCM = try await event(from: host, type: "pcm")
        XCTAssertEqual(finalPCM["audio"] as? String, "final")
        _ = try await event(from: host, type: "ended")
        captures[0].emit(["type":"error", "message":"late"])
        XCTAssertNil(host.poll())

        for operation in ["dictate", "start"] {
            XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
            authorizations.last?(true)
            let old = try XCTUnwrap(captures.last)
            XCTAssertTrue(host.command(#"{"operation":"finish"}"#))
            XCTAssertTrue(host.command("{\"operation\":\"\(operation)\"}"))
            XCTAssertGreaterThan(old.stops, 0, "Superseding must stop native audio before permission resolves")
            while host.poll() != nil {}
            for type in ["pcm", "level", "ended", "error"] {
                old.emit(["type":type, "audio":"stale", "message":"stale"])
            }
            XCTAssertNil(host.poll(), "Old native events must not enter the new capture")
            authorizations.last?(true)
            if operation == "start" { _ = try await event(from: host, type: "voice_authorized") }
            XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
            _ = try await event(from: host, type: "ended")
        }
        host.close()
        captures.last?.emit(["type":"pcm", "audio":"closed"])
        XCTAssertNil(host.poll())
    }

    func testDelayedAuthorizationCannotRestartAnOldCapture() async throws {
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(authorizationRequestForTesting: { authorizations.append($0) })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        XCTAssertEqual(authorizations.count, 1)
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        XCTAssertEqual(authorizations.count, 2)
        while host.poll() != nil {}
        authorizations[0](false)
        authorizations[0](true)
        XCTAssertNil(host.poll(), "Old permission results must not emit an error or open capture")
        authorizations[1](true)
        _ = try await event(from: host, type: "voice_authorized")
        host.close()
        authorizations[1](true)
        XCTAssertNil(host.poll())
    }

    func testQueuedAuthorizationCannotStartASupersedingCapture() throws {
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(authorizationRequestForTesting: { authorizations.append($0) })
        defer { host.close() }
        for operation in ["start", "dictate", "stop"] {
            XCTAssertTrue(host.command(#"{"operation":"start"}"#))
            authorizations.last?(true)
            XCTAssertTrue(host.command("{\"operation\":\"\(operation)\"}"))
            if operation == "stop" {
                let pointer = try XCTUnwrap(host.poll())
                let value = try XCTUnwrap(JSONSerialization.jsonObject(
                    with: Data(String(cString: pointer).utf8)) as? [String: Any])
                XCTAssertEqual(value["type"] as? String, "ended")
            }
            XCTAssertNil(host.poll(), "Queued authorization belongs to the superseded capture")
        }
    }

    func testClosedHostKeepsTerminalEventUntilPolled() throws {
        _ = NSApplication.shared
        var capture: DictationProbe?
        let host = VoiceMediaHost(authorizationRequestForTesting: { $0(true) },
                                  dictationFactoryForTesting: { emit in
            let value = DictationProbe(emit: emit)
            capture = value
            return value
        })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        let source = try XCTUnwrap(capture)
        for _ in 0..<129 { source.emit(["type":"pcm", "audio":"AAA="]) }
        XCTAssertGreaterThan(source.stops, 0)
        XCTAssertFalse(host.command(#"{"operation":"dictate"}"#), "Overflow must close the host")
        let pointer = try XCTUnwrap(host.poll())
        let event = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any])
        XCTAssertEqual(event["type"] as? String, "error")
        XCTAssertEqual(event["message"] as? String, "Audio updates could not be delivered. The call stopped.")
        host.close()
        XCTAssertNil(host.poll())
    }

    func testInputDiscoveryDoesNotRequestMicrophonePermission() throws {
        var authorizations = 0
        let host = VoiceMediaHost(authorizationRequestForTesting: { _ in authorizations += 1 })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"devices"}"#))
        let pointer = try XCTUnwrap(host.poll())
        let value = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any])
        XCTAssertEqual(value["type"] as? String, "devices")
        XCTAssertNotNil(value["inputs"] as? [String])
        XCTAssertEqual(authorizations, 0)
    }

    func testMediaABIRejectsDetachedViews() throws {
        _ = NSApplication.shared
        XCTAssertEqual(decodexVoiceMediaABIVersion(), 3)
        let detached = NSView()
        XCTAssertNil(decodexVoiceMediaCreate(Unmanaged.passUnretained(detached).toOpaque()))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.borderless], backing: .buffered, defer: false)
        let view = try XCTUnwrap(window.contentView)
        let pointer = try XCTUnwrap(decodexVoiceMediaCreate(Unmanaged.passUnretained(view).toOpaque()))
        decodexVoiceMediaDestroy(pointer)
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
        XCTFail("No native permission adapter event: \(type)")
        throw NSError(domain: "VoiceMediaHostTests", code: 2)
    }
}

@MainActor
private final class DictationProbe: DictationCapturing {
    let emit: @MainActor @Sendable ([String: Any]) -> Void
    var finishes = 0
    var stops = 0
    init(emit: @escaping @MainActor @Sendable ([String: Any]) -> Void) { self.emit = emit }
    func start(input: String) throws {}
    func finish() { finishes += 1 }
    func stop() { stops += 1 }
}
