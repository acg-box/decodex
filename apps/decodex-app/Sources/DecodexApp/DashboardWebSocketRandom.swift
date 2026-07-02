import Foundation
import Security

extension DashboardWebSocketConnection {
	func randomData(byteCount: Int) -> Data {
		var bytes = [UInt8](repeating: 0, count: byteCount)
		let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
		if status != errSecSuccess {
			for index in bytes.indices {
				bytes[index] = UInt8.random(in: UInt8.min...UInt8.max)
			}
		}

		return Data(bytes)
	}
}
