import XCTest
import Foundation
@testable import DecodexApp

@MainActor
final class VoiceMediaHostTests: XCTestCase {
    private func event(_ host: VoiceMediaHost) throws -> [String: Any] {
        let pointer = try XCTUnwrap(host.poll())
        return try XCTUnwrap(JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8)) as? [String: Any])
    }

    func testPermissionSelectsNativeCaptureWithoutStartingAudioInTheHost() throws {
        for operation in ["dictate", "start"] {
            let host = VoiceMediaHost(authorizationRequestForTesting: { $0(true) }, deviceForTesting: { 42 })
            defer { host.close() }
            XCTAssertTrue(host.command("{\"operation\":\"\(operation)\"}"))
            let value = try event(host)
            XCTAssertEqual(value["type"] as? String, operation == "dictate" ? "dictation_authorized" : "voice_authorized")
            XCTAssertEqual(value["device"] as? Int, 42)
            XCTAssertNil(host.poll())
        }
    }

    func testCancelledOrSupersededPermissionCannotStartCapture() throws {
        var authorizations: [@MainActor (Bool) -> Void] = []
        let host = VoiceMediaHost(authorizationRequestForTesting: { authorizations.append($0) }, deviceForTesting: { 42 })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        XCTAssertTrue(host.command(#"{"operation":"stop"}"#))
        _ = host.poll()
        authorizations[0](true)
        XCTAssertNil(host.poll())
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        XCTAssertTrue(host.command(#"{"operation":"start"}"#))
        authorizations[1](true)
        XCTAssertNil(host.poll())
        authorizations[2](true)
        XCTAssertEqual(try event(host)["type"] as? String, "voice_authorized")
    }

    func testDeniedPermissionReportsFailure() throws {
        let host = VoiceMediaHost(authorizationRequestForTesting: { $0(false) }, deviceForTesting: { XCTFail("Denied permission must not resolve devices"); return 42 })
        defer { host.close() }
        XCTAssertTrue(host.command(#"{"operation":"dictate"}"#))
        XCTAssertEqual(try event(host)["type"] as? String, "error")
    }
}
