import AppKit
import XCTest

@testable import DecodexApp

@MainActor
final class VoiceMediaHostTests: XCTestCase {
    func testNativeWebRTCProducesOfferAndStopsWithoutMicrophoneAccess() async throws {
        _ = NSApplication.shared
        let host = VoiceMediaHost(syntheticAudioForTesting: true)
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        let offer = try await event(from: host, type: "offer")
        let sdp = try XCTUnwrap(offer["sdp"] as? String)
        XCTAssertTrue(sdp.contains("m=audio"))
        XCTAssertTrue(sdp.contains("m=application"))
        XCTAssertTrue(host.command(#"{"operation":"mute","muted":true}"#))
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = try await event(from: host, type: "ended")
        host.close()
        XCTAssertFalse(host.command(#"{"operation":"start"}"#))
    }

    private func event(from host: VoiceMediaHost, type: String) async throws -> [String: Any] {
        let deadline = Date().addingTimeInterval(10)
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
        XCTFail("No native media event: \(type)")
        throw NSError(domain: "VoiceMediaHostTests", code: 2)
    }
}
