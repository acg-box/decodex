import Darwin
import Foundation

let decodexNativeClientSchema = "decodex/app-native-client/1"

private let decodexNativeClientConfigSchema = "decodex/app-native-client-config/1"
private let decodexNativeClientResponseLimit = 8 * 1024 * 1024

struct DecodexNativeRequest: Encodable, Sendable {
	let schema = decodexNativeClientSchema
	let operation: String
	var accountID: String? = nil
	var includeEmail: Bool? = nil
	var grantedAtUnixSeconds: Int64? = nil
	var expiresAtUnixSeconds: Int64? = nil
	var expectedRevision: UInt64? = nil
	var expectedRoutingRevision: UInt64? = nil
	var afterGeneration: UInt64? = nil
	var requestRefresh: Bool? = nil
	var order: [String]? = nil
	var idempotencyKey: String? = nil
	var operationID: String? = nil
	var recoveryOperationID: String? = nil
	var sessionID: String? = nil
	var loginMethod: AccountLoginMethod? = nil
	var enabled: Bool? = nil

	enum CodingKeys: String, CodingKey {
		case schema
		case operation
		case accountID = "account_id"
		case includeEmail = "include_email"
		case grantedAtUnixSeconds = "granted_at_unix_seconds"
		case expiresAtUnixSeconds = "expires_at_unix_seconds"
		case expectedRevision = "expected_revision"
		case expectedRoutingRevision = "expected_routing_revision"
		case afterGeneration = "after_generation"
		case requestRefresh = "request_refresh"
		case order
		case idempotencyKey = "idempotency_key"
		case operationID = "operation_id"
		case recoveryOperationID = "recovery_operation_id"
		case sessionID = "session_id"
		case loginMethod = "login_method"
		case enabled
	}
}

struct DecodexNativeAuthorityWire: Decodable, Sendable {
	let profileName: String
	let serverID: String

	private enum CodingKeys: String, CodingKey {
		case profileName = "profile_name"
		case serverID = "server_id"
	}

	init(from decoder: Decoder) throws {
		try requireExactFields(
			in: decoder,
			expected: ["profile_name", "server_id"]
		)
		let container = try decoder.container(keyedBy: CodingKeys.self)
		profileName = try container.decode(String.self, forKey: .profileName)
		serverID = try container.decode(String.self, forKey: .serverID)
	}

	var value: ResetCardAuthority {
		get throws {
			let value = ResetCardAuthority(
				profileName: profileName,
				serverID: serverID
			)
			guard DecodexNativeClient.isValidAuthority(value) else {
				throw ResetCardClientError.invalidResponse
			}
			return value
		}
	}
}

enum DecodexNativeFailure: String, Decodable, Sendable {
	case configurationMissing = "configuration_missing"
	case configurationMalformed = "configuration_malformed"
	case configurationVersion = "configuration_version"
	case profileMissing = "profile_missing"
	case unsafeHostPath = "unsafe_host_path"
	case serverIdentityUnavailable = "server_identity_unavailable"
	case remoteMutationUnsupported = "remote_mutation_unsupported"
	case localTransportDisabled = "local_transport_disabled"
	case remoteTransportDisabled = "remote_transport_disabled"
	case localTransportUnsupported = "local_transport_unsupported"
	case unsafeLocalEndpoint = "unsafe_local_endpoint"
	case localPeerIdentityUnavailable = "local_peer_identity_unavailable"
	case localPeerUIDMismatch = "local_peer_uid_mismatch"
	case protocolDisconnected = "protocol_disconnected"
	case protocolTimeout = "protocol_timeout"
	case protocolMajorMismatch = "protocol_major_mismatch"
	case protocolMinorMismatch = "protocol_minor_mismatch"
	case serviceVersionMismatch = "service_version_mismatch"
	case serverIdentityMismatch = "server_identity_mismatch"
	case protocolMalformed = "protocol_malformed"
	case protocolViolation = "protocol_violation"
	case protocolBackpressure = "protocol_backpressure"
	case applicationAcceptanceUnknown = "application_acceptance_unknown"
	case invalidConfiguration = "invalid_configuration"
	case invalidRequest = "invalid_request"
	case invalidInput = "invalid_input"
	case invalidHandle = "invalid_handle"
	case runtimeUnavailable = "runtime_unavailable"
	case internalFailure = "internal_failure"
	case homeUnavailable = "home_unavailable"
	case unsafeConfigPath = "unsafe_config_path"
	case configUnavailable = "config_unavailable"
	case configTooLarge = "config_too_large"
	case configInvalid = "config_invalid"
	case featuresNotTable = "features_not_table"
	case fastModeNotBoolean = "fast_mode_not_boolean"
	case writeFailed = "write_failed"

	var clientError: ResetCardClientError {
		switch self {
		case .protocolTimeout:
			return .timedOut
		case .protocolDisconnected:
			return .transportDisconnected
		case .protocolBackpressure:
			return .transportBackpressured
		case .runtimeUnavailable, .invalidHandle, .internalFailure,
			.serviceVersionMismatch:
			return .nativeClientUnavailable
		case .applicationAcceptanceUnknown:
			return .usePotentiallyDispatched
		case .homeUnavailable, .unsafeConfigPath, .configUnavailable, .configTooLarge,
			.configInvalid, .featuresNotTable, .fastModeNotBoolean, .writeFailed:
			return .commandRejected
		case .configurationMissing, .configurationMalformed, .configurationVersion,
			.profileMissing, .unsafeHostPath, .serverIdentityUnavailable,
			.remoteMutationUnsupported, .localTransportDisabled, .remoteTransportDisabled,
			.localTransportUnsupported, .unsafeLocalEndpoint, .localPeerIdentityUnavailable,
			.localPeerUIDMismatch, .protocolMajorMismatch, .protocolMinorMismatch,
			.serverIdentityMismatch, .protocolMalformed, .protocolViolation,
			.invalidConfiguration, .invalidRequest, .invalidInput:
			return .invalidResponse
		}
	}
}

private enum DecodexNativeResponse<Payload: Decodable> {
	case success(
		operation: String,
		authority: DecodexNativeAuthorityWire,
		data: Payload
	)
	case failure(operation: String, DecodexNativeFailure)
}

extension DecodexNativeResponse: Decodable {
	private enum CodingKeys: String, CodingKey {
		case schema
		case outcome
		case operation
		case authority
		case data
		case failure
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		let schema = try container.decode(String.self, forKey: .schema)
		guard schema == decodexNativeClientSchema else {
			throw ResetCardClientError.invalidResponse
		}
		let operation = try container.decode(String.self, forKey: .operation)
		guard isBoundedWireText(operation, maximumBytes: 64) else {
			throw ResetCardClientError.invalidResponse
		}

		switch try container.decode(String.self, forKey: .outcome) {
		case "success":
			try requireExactFields(
				in: decoder,
				expected: ["schema", "outcome", "operation", "authority", "data"]
			)
			self = .success(
				operation: operation,
				authority: try container.decode(
					DecodexNativeAuthorityWire.self,
					forKey: .authority
				),
				data: try container.decode(Payload.self, forKey: .data)
			)
		case "failure":
			try requireExactFields(
				in: decoder,
				expected: ["schema", "outcome", "operation", "failure"]
			)
			self = .failure(
				operation: operation,
				try container.decode(DecodexNativeFailure.self, forKey: .failure)
			)
		default:
			throw ResetCardClientError.invalidResponse
		}
	}
}

final class DecodexNativeClient: @unchecked Sendable, CustomDebugStringConvertible {
	typealias TestRequest = @Sendable (Data, ResetCardAuthority?) async throws -> Data

	private static let sharedSession = DecodexNativeSession()

	private let request: TestRequest
	private let authorityLock = NSLock()
	private var establishedAuthority: ResetCardAuthority?

	init() {
		request = { data, authority in
			try await Task.detached {
				try Self.sharedSession.request(data, authority: authority)
			}.value
		}
	}

	init(request: @escaping TestRequest) {
		self.request = request
	}

	var debugDescription: String {
		"DecodexNativeClient(transport: in-process)"
	}

	static func shutdownSharedSession() async {
		await Task.detached {
			sharedSession.shutdown()
		}.value
	}

	func perform<Payload: Decodable>(
		_ request: DecodexNativeRequest,
		authority requestedAuthority: ResetCardAuthority?,
		as payloadType: Payload.Type = Payload.self
	) async throws -> (authority: ResetCardAuthority, data: Payload) {
		guard requestedAuthority.map(Self.isValidAuthority) ?? true else {
			throw ResetCardClientError.invalidResponse
		}
		let currentAuthority = authorityLock.withLock { establishedAuthority }
		guard currentAuthority == nil
			|| requestedAuthority == nil
			|| currentAuthority == requestedAuthority
		else {
			throw ResetCardClientError.invalidResponse
		}

		let requestData: Data
		do {
			requestData = try JSONEncoder().encode(request)
		} catch {
			throw ResetCardClientError.invalidResponse
		}
		let responseData = try await self.request(
			requestData,
			requestedAuthority ?? currentAuthority
		)
		guard responseData.count <= decodexNativeClientResponseLimit else {
			throw ResetCardClientError.outputTooLarge
		}

		let response: DecodexNativeResponse<Payload>
		do {
			response = try JSONDecoder().decode(
				DecodexNativeResponse<Payload>.self,
				from: responseData
			)
		} catch let error as ResetCardClientError {
			throw error
		} catch {
			throw ResetCardClientError.invalidResponse
		}

		switch response {
		case .failure(let operation, let failure):
			guard operation == request.operation else {
				throw ResetCardClientError.invalidResponse
			}
			throw failure.clientError
		case .success(let operation, let authorityWire, let data):
			guard operation == request.operation else {
				throw ResetCardClientError.invalidResponse
			}
			let actualAuthority = try authorityWire.value
			guard requestedAuthority.map({ $0 == actualAuthority }) ?? true,
				currentAuthority.map({ $0 == actualAuthority }) ?? true
			else {
				throw ResetCardClientError.invalidResponse
			}
			authorityLock.withLock {
				establishedAuthority = actualAuthority
			}
			return (actualAuthority, data)
		}
	}

	static func isCanonicalAccountID(_ value: String) -> Bool {
		isCanonicalUUID(value)
	}

	static func isCanonicalUUID(_ value: String) -> Bool {
		guard let uuid = UUID(uuidString: value) else {
			return false
		}
		return uuid.uuidString.lowercased() == value
	}

	static func isValidAuthority(_ authority: ResetCardAuthority) -> Bool {
		let profile = authority.profileName
		return profile.isEmpty == false
			&& profile.utf8.count <= 64
			&& profile.utf8.allSatisfy {
				($0 >= 0x61 && $0 <= 0x7a)
					|| ($0 >= 0x41 && $0 <= 0x5a)
					|| ($0 >= 0x30 && $0 <= 0x39)
					|| $0 == 0x2d
					|| $0 == 0x5f
			}
			&& isCanonicalUUID(authority.serverID)
	}
}

private final class DecodexNativeSession: @unchecked Sendable {
	private let lock = NSLock()
	private var client: UnsafeMutableRawPointer?
	private var authority: ResetCardAuthority?
	private var closed = false

	func request(
		_ requestData: Data,
		authority requestedAuthority: ResetCardAuthority?
	) throws -> Data {
		let library = try DecodexNativeLibrary.shared.get()
		let handle = try lock.withLock {
			guard closed == false,
				authority == nil
				|| requestedAuthority == nil
				|| authority == requestedAuthority
			else {
				throw ResetCardClientError.invalidResponse
			}
			if let client {
				return client
			}

			let config = DecodexNativeConfig(
				schema: decodexNativeClientConfigSchema,
				profileName: requestedAuthority?.profileName,
				expectedServerID: requestedAuthority?.serverID
			)
			let configData: Data
			do {
				configData = try JSONEncoder().encode(config)
			} catch {
				throw ResetCardClientError.invalidResponse
			}
			var errorBuffer: UnsafeMutablePointer<UInt8>?
			var errorLength = 0
			let created = configData.withUnsafeBytes { bytes in
				library.create(
					bytes.bindMemory(to: UInt8.self).baseAddress,
					configData.count,
					&errorBuffer,
					&errorLength
				)
			}
			if let errorBuffer, errorLength > 0 {
				library.free(errorBuffer, errorLength)
			}
			guard let created else {
				throw ResetCardClientError.nativeClientUnavailable
			}
			client = created
			authority = requestedAuthority
			return created
		}

		var responseBuffer: UnsafeMutablePointer<UInt8>?
		var responseLength = 0
		let status = requestData.withUnsafeBytes { bytes in
			library.request(
				handle,
				bytes.bindMemory(to: UInt8.self).baseAddress,
				requestData.count,
				&responseBuffer,
				&responseLength
			)
		}
		guard status == 0,
			let responseBuffer,
			responseLength > 0,
			responseLength <= decodexNativeClientResponseLimit
		else {
			if let responseBuffer, responseLength > 0 {
				library.free(responseBuffer, responseLength)
			}
			if responseLength > decodexNativeClientResponseLimit {
				throw ResetCardClientError.outputTooLarge
			}
			throw ResetCardClientError.nativeClientUnavailable
		}
		defer {
			library.free(responseBuffer, responseLength)
		}
		return Data(bytes: responseBuffer, count: responseLength)
	}

	func shutdown() {
		let handle = lock.withLock {
			closed = true
			defer {
				client = nil
				authority = nil
			}
			return client
		}
		if let handle, let library = try? DecodexNativeLibrary.shared.get() {
			library.destroy(handle)
		}
	}

	deinit {
		shutdown()
	}
}

private struct DecodexNativeConfig: Encodable {
	let schema: String
	let profileName: String?
	let expectedServerID: String?

	private enum CodingKeys: String, CodingKey {
		case schema
		case profileName = "profile_name"
		case expectedServerID = "expected_server_id"
	}
}

private final class DecodexNativeLibrary: @unchecked Sendable {
	typealias Create = @convention(c) (
		UnsafePointer<UInt8>?,
		Int,
		UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
		UnsafeMutablePointer<Int>?
	) -> UnsafeMutableRawPointer?
	typealias Request = @convention(c) (
		UnsafeMutableRawPointer?,
		UnsafePointer<UInt8>?,
		Int,
		UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
		UnsafeMutablePointer<Int>?
	) -> Int32
	typealias Free = @convention(c) (UnsafeMutablePointer<UInt8>?, Int) -> Void
	typealias Destroy = @convention(c) (UnsafeMutableRawPointer?) -> Void

	static let shared = ResultBox()

	let create: Create
	let request: Request
	let free: Free
	let destroy: Destroy

	private let image: UnsafeMutableRawPointer

	init() throws {
		guard let frameworksURL = Bundle.main.privateFrameworksURL else {
			throw ResetCardClientError.nativeClientUnavailable
		}
		let libraryURL = frameworksURL.appendingPathComponent(
			"libdecodex_app_client_ffi.dylib",
			isDirectory: false
		)
		let image: UnsafeMutableRawPointer
		do {
			image = try DecodexNativeCompatibility.openLibrary(at: libraryURL)
		} catch {
			throw ResetCardClientError.nativeClientUnavailable
		}
		self.image = image

		do {
			create = try Self.symbol(
				image,
				named: "decodex_app_native_client_create"
			)
			request = try Self.symbol(
				image,
				named: "decodex_app_native_client_request"
			)
			free = try Self.symbol(
				image,
				named: "decodex_app_native_client_free"
			)
			destroy = try Self.symbol(
				image,
				named: "decodex_app_native_client_destroy"
			)
		} catch {
			dlclose(image)
			throw error
		}
	}

	deinit {
		dlclose(image)
	}

	private static func symbol<T>(
		_ image: UnsafeMutableRawPointer,
		named name: String
	) throws -> T {
		guard let value = dlsym(image, name) else {
			throw ResetCardClientError.nativeClientUnavailable
		}
		return unsafeBitCast(value, to: T.self)
	}

	final class ResultBox: @unchecked Sendable {
		private let lock = NSLock()
		private var cached: Result<DecodexNativeLibrary, Error>?

		func get() throws -> DecodexNativeLibrary {
			try lock.withLock {
				if let cached {
					return try cached.get()
				}
				let result = Result { try DecodexNativeLibrary() }
				cached = result
				return try result.get()
			}
		}
	}
}
