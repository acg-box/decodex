import Darwin
import Foundation
@testable import DecodexApp

/// Tests load the same Rust owner from the artifact built by test_native_app.sh.
private final class JournalLibrary: @unchecked Sendable {
	typealias Request = @convention(c) (UnsafePointer<UInt8>?, Int, UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?, UnsafeMutablePointer<Int>?) -> Int32
	typealias Free = @convention(c) (UnsafeMutablePointer<UInt8>?, Int) -> Void
	let image: UnsafeMutableRawPointer
	let request: Request
	let free: Free
	init() throws {
		guard let path = ProcessInfo.processInfo.environment["DECODEX_TEST_NATIVE_LIBRARY"] else {
			throw NSError(domain: "Run scripts/macos/test_native_app.sh to build the native test dependency", code: 1)
		}
		image = try DecodexNativeCompatibility.openLibrary(at: URL(fileURLWithPath: path))
		guard let requestSymbol = dlsym(image, "decodex_reset_card_journal_v1"),
			let freeSymbol = dlsym(image, "decodex_app_native_client_free") else {
			dlclose(image)
			throw NSError(domain: "Native journal symbols missing", code: 2)
		}
		request = unsafeBitCast(requestSymbol, to: Request.self)
		free = unsafeBitCast(freeSymbol, to: Free.self)
	}
	deinit { dlclose(image) }
}

enum NativeJournalFixture {
	private static let library = Result { try JournalLibrary() }
	static func request(_ data: Data) throws -> Data {
		let library = try library.get()
		var buffer: UnsafeMutablePointer<UInt8>?
		var length = 0
		let status = data.withUnsafeBytes { bytes in
			library.request(bytes.bindMemory(to: UInt8.self).baseAddress, data.count, &buffer, &length)
		}
		defer { if let buffer { library.free(buffer, length) } }
		guard status == 0, let buffer, length > 0 else { throw ResetCardClientError.nativeClientUnavailable }
		return Data(bytes: buffer, count: length)
	}
}
