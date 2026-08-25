import Darwin
import Foundation

enum ResetCardPendingAttemptLoad: Equatable {
	case available([ResetCardUseAttempt])
	case recoveryBlocked([ResetCardUseAttempt])

	var attempts: [ResetCardUseAttempt] {
		switch self {
		case let .available(attempts), let .recoveryBlocked(attempts):
			return attempts
		}
	}

	var isRecoveryBlocked: Bool {
		if case .recoveryBlocked = self {
			return true
		}

		return false
	}
}

enum ResetCardPendingDispatchJournalUpdate: Equatable {
	case retained
	case removed
	case removalFailed
}

struct ResetCardPendingDispatchResult<Value> {
	let value: Value
	let journalUpdate: ResetCardPendingDispatchJournalUpdate
}

@MainActor
struct ResetCardPendingAttemptStore {
	private static let schema = "decodex/reset-card-pending/2"
	static let maximumAttempts = 64
	private static let maximumDocumentBytes = 64 * 1_024
	private static var inProcessLockKeys = Set<String>()

	let journalURL: URL

	init(journalURL: URL = ResetCardPendingAttemptStore.defaultJournalURL()) {
		self.journalURL = journalURL
	}

	func load() -> ResetCardPendingAttemptLoad {
		let journal = readJournal()
		guard case .data(let data) = journal else {
			if case .missing = journal {
				return .available([])
			}

			return .recoveryBlocked([])
		}
		guard let document = try? JSONDecoder().decode(Document.self, from: data) else {
			return .recoveryBlocked([])
		}

		let recovered = Self.recoverableAttempts(from: document.attempts)
		let isValid = document.schema == Self.schema
			&& document.attempts.count <= Self.maximumAttempts
			&& recovered == document.attempts
			&& Set(document.attempts.map(LogicalTarget.init)).count
				== document.attempts.count

		return isValid ? .available(recovered) : .recoveryBlocked(recovered)
	}

	private func readJournal() -> JournalRead {
		switch directoryState() {
		case .missing:
			return .missing
		case .unsafe:
			return .unsafe
		case .available:
			break
		}

		var pathStatus = Darwin.stat()
		guard Darwin.lstat(journalURL.path, &pathStatus) == 0 else {
			return errno == ENOENT ? .missing : .unsafe
		}
		guard Self.isPrivateRegularFile(
			pathStatus,
			maximumBytes: Self.maximumDocumentBytes
		),
			pathStatus.st_nlink == 1
		else {
			return .unsafe
		}

		let descriptor = Darwin.open(
			journalURL.path,
			O_RDONLY | O_CLOEXEC | O_NOFOLLOW
		)
		guard descriptor >= 0 else {
			return .unsafe
		}
		defer {
			_ = Darwin.close(descriptor)
		}

		var openedStatus = Darwin.stat()
		guard Darwin.fstat(descriptor, &openedStatus) == 0,
			Self.isPrivateRegularFile(
				openedStatus,
				maximumBytes: Self.maximumDocumentBytes
			),
			openedStatus.st_nlink == 1,
			Self.isSameFile(pathStatus, openedStatus),
			let data = Self.readAll(
				from: descriptor,
				expectedBytes: Int(openedStatus.st_size),
				maximumBytes: Self.maximumDocumentBytes
			)
		else {
			return .unsafe
		}

		var finalStatus = Darwin.stat()
		guard Darwin.fstat(descriptor, &finalStatus) == 0,
			Self.isPrivateRegularFile(
				finalStatus,
				maximumBytes: Self.maximumDocumentBytes
			),
			finalStatus.st_nlink == 1,
			Self.isSameFile(openedStatus, finalStatus),
			finalStatus.st_size == openedStatus.st_size
		else {
			return .unsafe
		}

		return .data(data)
	}

	private func directoryState() -> DirectoryState {
		let directory = journalURL.deletingLastPathComponent()
		var status = Darwin.stat()
		guard Darwin.lstat(directory.path, &status) == 0 else {
			return errno == ENOENT ? .missing : .unsafe
		}

		return Self.isPrivateDirectory(status) ? .available : .unsafe
	}

	private enum JournalRead {
		case missing
		case unsafe
		case data(Data)
	}

	private enum DirectoryState {
		case missing
		case unsafe
		case available
	}

	func insert(_ attempt: ResetCardUseAttempt) -> [ResetCardUseAttempt]? {
		withExclusiveLock {
			guard Self.isValid(attempt),
				case .available(let current) = load()
			else {
				return nil
			}
			if let existing = current.first(where: {
				$0.idempotencyKey == attempt.idempotencyKey
			}) {
				return existing == attempt ? current : nil
			}
			guard current.contains(where: {
				$0.target.accountID == attempt.target.accountID
					&& $0.target.descriptor == attempt.target.descriptor
			}) == false else {
				return nil
			}
			guard current.count < Self.maximumAttempts else {
				return nil
			}

			let updated = current + [attempt]
			guard let data = Self.encode(updated),
				persist(data)
			else {
				return nil
			}

			return updated
		}
	}

	func remove(_ attempt: ResetCardUseAttempt) -> [ResetCardUseAttempt]? {
		withExclusiveLock {
			guard case .available(let current) = load() else {
				return nil
			}
			if let existing = current.first(where: {
				$0.idempotencyKey == attempt.idempotencyKey
			}),
				existing != attempt
			{
				return nil
			}

			let updated = current.filter {
				$0.idempotencyKey != attempt.idempotencyKey
			}
			guard updated != current else {
				return current
			}
			guard let data = Self.encode(updated),
				persist(data)
			else {
				return nil
			}

			return updated
		}
	}

	func withDispatchLock<Value>(
		for attempt: ResetCardUseAttempt,
		operation: () async -> Value,
		shouldRemove: (Value) -> Bool
	) async -> ResetCardPendingDispatchResult<Value>? {
		guard let inProcessLockKey = acquireInProcessLock() else {
			return nil
		}
		defer {
			releaseInProcessLock(inProcessLockKey)
		}
		guard let descriptor = acquireLock(command: F_SETLK) else {
			return nil
		}
		defer {
			releaseLock(descriptor)
		}
		guard case .available(let current) = load(),
			current.contains(attempt)
		else {
			return nil
		}

		let value = await operation()
		guard shouldRemove(value) else {
			return ResetCardPendingDispatchResult(
				value: value,
				journalUpdate: .retained
			)
		}

		let updated = current.filter {
			$0.idempotencyKey != attempt.idempotencyKey
		}
		guard let data = Self.encode(updated),
			persist(data)
		else {
			return ResetCardPendingDispatchResult(
				value: value,
				journalUpdate: .removalFailed
			)
		}

		return ResetCardPendingDispatchResult(
			value: value,
			journalUpdate: .removed
		)
	}

	private func persist(_ data: Data) -> Bool {
		let fileManager = FileManager.default
		let directory = journalURL.deletingLastPathComponent()

		guard ensureDirectory() else {
			return false
		}

		let temporaryURL = directory.appendingPathComponent(
			".\(journalURL.lastPathComponent).\(UUID().uuidString).tmp",
			isDirectory: false
		)
		let descriptor = Darwin.open(
			temporaryURL.path,
			O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
			S_IRUSR | S_IWUSR
		)
		guard descriptor >= 0 else {
			return false
		}

		var descriptorIsOpen = true
		var renamed = false
		defer {
			if descriptorIsOpen {
				_ = Darwin.close(descriptor)
			}
			if renamed == false {
				try? fileManager.removeItem(at: temporaryURL)
			}
		}

		var temporaryStatus = Darwin.stat()
		guard Darwin.fchmod(descriptor, S_IRUSR | S_IWUSR) == 0,
			Darwin.fstat(descriptor, &temporaryStatus) == 0,
			Self.isPrivateRegularFile(temporaryStatus),
			temporaryStatus.st_nlink == 1,
			Self.writeAll(data, to: descriptor),
			Self.synchronizeFile(descriptor)
		else {
			return false
		}
		let closeResult = Darwin.close(descriptor)
		descriptorIsOpen = false
		guard closeResult == 0 else {
			return false
		}

		guard Darwin.rename(temporaryURL.path, journalURL.path) == 0 else {
			return false
		}
		renamed = true

		guard Self.synchronizeDirectory(directory),
			case .data(let readback) = readJournal(),
			readback == data
		else {
			return false
		}

		return true
	}

	private func withExclusiveLock<Result>(
		_ operation: () -> Result?
	) -> Result? {
		guard let inProcessLockKey = acquireInProcessLock() else {
			return nil
		}
		defer {
			releaseInProcessLock(inProcessLockKey)
		}
		guard let descriptor = acquireLock(command: F_SETLKW) else {
			return nil
		}
		defer {
			releaseLock(descriptor)
		}

		return operation()
	}

	private func acquireInProcessLock() -> String? {
		let key = journalURL.resolvingSymlinksInPath().standardizedFileURL.path
		guard Self.inProcessLockKeys.insert(key).inserted else {
			return nil
		}

		return key
	}

	private func releaseInProcessLock(_ key: String) {
		Self.inProcessLockKeys.remove(key)
	}

	private func acquireLock(command: Int32) -> Int32? {
		guard ensureDirectory() else {
			return nil
		}

		let lockURL = journalURL.deletingLastPathComponent().appendingPathComponent(
			".\(journalURL.lastPathComponent).lock",
			isDirectory: false
		)
		var created = true
		var descriptor = Darwin.open(
			lockURL.path,
			O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
			S_IRUSR | S_IWUSR
		)
		if descriptor < 0, errno == EEXIST {
			created = false
			descriptor = Darwin.open(
				lockURL.path,
				O_RDWR | O_CLOEXEC | O_NOFOLLOW
			)
		}
		guard descriptor >= 0 else {
			return nil
		}
		if created {
			guard Darwin.fchmod(descriptor, S_IRUSR | S_IWUSR) == 0 else {
				_ = Darwin.close(descriptor)
				return nil
			}
		}
		var descriptorStatus = Darwin.stat()
		var pathStatus = Darwin.stat()
		guard Darwin.fstat(descriptor, &descriptorStatus) == 0,
			Darwin.lstat(lockURL.path, &pathStatus) == 0,
			Self.isPrivateRegularFile(descriptorStatus),
			Self.isPrivateRegularFile(pathStatus),
			descriptorStatus.st_nlink == 1,
			pathStatus.st_nlink == 1,
			Self.isSameFile(descriptorStatus, pathStatus)
		else {
			_ = Darwin.close(descriptor)
			return nil
		}
		guard Self.setFileLock(
			descriptor,
			type: Int16(F_WRLCK),
			command: command
		) else {
			_ = Darwin.close(descriptor)
			return nil
		}

		return descriptor
	}

	private func releaseLock(_ descriptor: Int32) {
		_ = Self.setFileLock(descriptor, type: Int16(F_UNLCK), command: F_SETLK)
		_ = Darwin.close(descriptor)
	}

	private static func setFileLock(
		_ descriptor: Int32,
		type: Int16,
		command: Int32
	) -> Bool {
		var lock = Darwin.flock()
		lock.l_type = type
		lock.l_whence = Int16(SEEK_SET)
		lock.l_start = 0
		lock.l_len = 0

		while true {
			if Darwin.fcntl(descriptor, command, &lock) == 0 {
				return true
			}
			if errno != EINTR {
				return false
			}
		}
	}

	private func ensureDirectory() -> Bool {
		let fileManager = FileManager.default
		let directory = journalURL.deletingLastPathComponent()
		var pathStatus = Darwin.stat()
		var created = false

		if Darwin.lstat(directory.path, &pathStatus) != 0 {
			guard errno == ENOENT else {
				return false
			}
			do {
				try fileManager.createDirectory(
					at: directory,
					withIntermediateDirectories: true,
					attributes: [.posixPermissions: 0o700]
				)
				created = true
			} catch {
				return false
			}
		}

		let descriptor = Darwin.open(
			directory.path,
			O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW
		)
		guard descriptor >= 0 else {
			return false
		}
		defer {
			_ = Darwin.close(descriptor)
		}
		if created {
			guard Darwin.fchmod(descriptor, 0o700) == 0 else {
				return false
			}
		}

		var descriptorStatus = Darwin.stat()
		guard Darwin.fstat(descriptor, &descriptorStatus) == 0,
			Darwin.lstat(directory.path, &pathStatus) == 0,
			Self.isPrivateDirectory(descriptorStatus),
			Self.isPrivateDirectory(pathStatus),
			Self.isSameFile(descriptorStatus, pathStatus)
		else {
			return false
		}

		return true
	}

	private static func encode(_ attempts: [ResetCardUseAttempt]) -> Data? {
		let document = Document(
			schema: Self.schema,
			attempts: attempts
		)
		guard let data = try? JSONEncoder().encode(document),
			data.count <= Self.maximumDocumentBytes
		else {
			return nil
		}

		return data
	}

	private static func recoverableAttempts(
		from attempts: [ResetCardUseAttempt]
	) -> [ResetCardUseAttempt] {
		var grouped = [String: [ResetCardUseAttempt]]()
		for attempt in attempts where Self.isValid(attempt) {
			grouped[attempt.idempotencyKey, default: []].append(attempt)
		}

		var recovered = [ResetCardUseAttempt]()
		var seen = Set<String>()
		for attempt in attempts {
			guard recovered.count < Self.maximumAttempts,
				Self.isValid(attempt),
				seen.insert(attempt.idempotencyKey).inserted,
				let sameKey = grouped[attempt.idempotencyKey],
				sameKey.allSatisfy({ $0 == attempt })
			else {
				continue
			}
			recovered.append(attempt)
		}

		return recovered
	}

	private static func isValid(_ attempt: ResetCardUseAttempt) -> Bool {
		let target = attempt.target
		let descriptor = target.descriptor

		return DecodexNativeClient.isValidAuthority(target.authority)
			&& DecodexNativeClient.isCanonicalAccountID(target.accountID)
			&& target.expectedRevision > 0
			&& descriptor.grantedAtUnixSeconds >= 0
			&& descriptor.expiresAtUnixSeconds > descriptor.grantedAtUnixSeconds
			&& DecodexNativeClient.isCanonicalUUID(attempt.idempotencyKey)
	}

	private static func isPrivateRegularFile(
		_ status: Darwin.stat,
		maximumBytes: Int? = nil
	) -> Bool {
		let fileType = status.st_mode & mode_t(S_IFMT)
		let permissions = status.st_mode & mode_t(0o7777)

		guard fileType == mode_t(S_IFREG),
			status.st_uid == Darwin.geteuid(),
			permissions == mode_t(0o600),
			status.st_size >= 0
		else {
			return false
		}
		if let maximumBytes {
			return status.st_size <= off_t(maximumBytes)
		}

		return true
	}

	private static func isPrivateDirectory(_ status: Darwin.stat) -> Bool {
		let fileType = status.st_mode & mode_t(S_IFMT)
		let permissions = status.st_mode & mode_t(0o7777)

		return fileType == mode_t(S_IFDIR)
			&& status.st_uid == Darwin.geteuid()
			&& permissions == mode_t(0o700)
	}

	private static func isSameFile(
		_ first: Darwin.stat,
		_ second: Darwin.stat
	) -> Bool {
		first.st_dev == second.st_dev
			&& first.st_ino == second.st_ino
	}

	private static func readAll(
		from descriptor: Int32,
		expectedBytes: Int,
		maximumBytes: Int
	) -> Data? {
		guard expectedBytes >= 0, expectedBytes <= maximumBytes else {
			return nil
		}

		var data = Data()
		data.reserveCapacity(expectedBytes)
		var buffer = [UInt8](repeating: 0, count: 4_096)

		while true {
			let readCount = buffer.withUnsafeMutableBytes { bytes in
				Darwin.read(descriptor, bytes.baseAddress, bytes.count)
			}
			if readCount < 0 {
				if errno == EINTR {
					continue
				}
				return nil
			}
			if readCount == 0 {
				break
			}
			guard data.count + readCount <= maximumBytes else {
				return nil
			}
			data.append(contentsOf: buffer.prefix(readCount))
		}

		return data.count == expectedBytes ? data : nil
	}

	private static func writeAll(_ data: Data, to descriptor: Int32) -> Bool {
		data.withUnsafeBytes { bytes in
			guard let baseAddress = bytes.baseAddress else {
				return true
			}

			var offset = 0
			while offset < bytes.count {
				let written = Darwin.write(
					descriptor,
					baseAddress.advanced(by: offset),
					bytes.count - offset
				)
				if written < 0 {
					if errno == EINTR {
						continue
					}
					return false
				}
				guard written > 0 else {
					return false
				}
				offset += written
			}

			return true
		}
	}

	private static func synchronizeFile(_ descriptor: Int32) -> Bool {
		if Darwin.fcntl(descriptor, F_FULLFSYNC) == 0 {
			return true
		}

		return Darwin.fsync(descriptor) == 0
	}

	private static func synchronizeDirectory(_ directory: URL) -> Bool {
		let descriptor = Darwin.open(
			directory.path,
			O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW
		)
		guard descriptor >= 0 else {
			return false
		}
		defer {
			_ = Darwin.close(descriptor)
		}
		var status = Darwin.stat()
		guard Darwin.fstat(descriptor, &status) == 0,
			Self.isPrivateDirectory(status)
		else {
			return false
		}

		return Darwin.fsync(descriptor) == 0
	}

	private static func defaultJournalURL() -> URL {
		let base = FileManager.default.urls(
			for: .applicationSupportDirectory,
			in: .userDomainMask
		).first ?? FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Library/Application Support", isDirectory: true)

		return base
			.appendingPathComponent("Decodex", isDirectory: true)
			.appendingPathComponent("reset-card-pending-v1.json", isDirectory: false)
	}

	private struct Document: Codable {
		let schema: String
		let attempts: [ResetCardUseAttempt]
	}

	private struct LogicalTarget: Hashable {
		let accountID: String
		let descriptor: ResetCardDescriptor

		init(_ attempt: ResetCardUseAttempt) {
			accountID = attempt.target.accountID
			descriptor = attempt.target.descriptor
		}
	}
}
