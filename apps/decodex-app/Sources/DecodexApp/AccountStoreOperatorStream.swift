import Foundation
import OSLog

private let accountStoreLog = Logger(subsystem: "ink.hack.DecodexApp", category: "AccountStore")
private let operatorSnapshotReconnectInitialDelay: UInt64 = 1_000_000_000
private let operatorSnapshotReconnectMaxDelay: UInt64 = 30_000_000_000

extension AccountStore {
	func startOperatorSnapshotStream() {
		guard operatorSnapshotStreamTask == nil else {
			return
		}

		operatorSnapshotStreamTask = makeOperatorSnapshotStreamTask()
	}

	private func makeOperatorSnapshotStreamTask() -> Task<Void, Never> {
		Task { [weak self] in
			await self?.runOperatorSnapshotStream()
		}
	}

	private func runOperatorSnapshotStream() async {
		var reconnectDelay = operatorSnapshotReconnectInitialDelay

		while Task.isCancelled == false {
			do {
				try await connectOperatorSnapshotStream()
				reconnectDelay = operatorSnapshotReconnectInitialDelay
			} catch {
				accountStoreLog.warning("Operator snapshot stream dropped: \(error.localizedDescription, privacy: .public)")
			}

			do {
				try await Task.sleep(nanoseconds: reconnectDelay)
			} catch {
				return
			}
			reconnectDelay = min(operatorSnapshotReconnectMaxDelay, reconnectDelay * 2)
		}
	}

	private func connectOperatorSnapshotStream() async throws {
		let url = try await DecodexServerBridge.shared.dashboardWebSocketURL()
		let socket = DashboardWebSocketConnection(url: url)

		try await withTaskCancellationHandler {
			do {
				try await socket.connect()
				while Task.isCancelled == false {
					let data = try await socket.readMessageData()
					do {
						let event = try JSONDecoder().decode(OperatorDashboardSocketEvent.self, from: data)
						applyOperatorDashboardEvent(event)
					} catch {
						accountStoreLog.debug("Skipped dashboard WebSocket message bytes=\(data.count, privacy: .public) error=\(error.localizedDescription, privacy: .public)")
						continue
					}
				}
				await socket.close()
			} catch {
				await socket.close()
				throw error
			}
		} onCancel: {
			Task {
				await socket.close()
			}
		}
	}

	func applyOperatorDashboardEvent(_ event: OperatorDashboardSocketEvent) {
		guard let payload = event.payload else {
			return
		}

		switch event.type {
		case "snapshot":
			guard let snapshot = payload.snapshot else {
				return
			}

			operatorSnapshot = snapshot
			operatorPresentation = snapshot.presentation
			operatorSnapshotPublishedAtUnixEpoch = payload.snapshotPublishedAtUnixEpoch
			operatorSnapshotUpdatedAt = payload.snapshotPublishedAt ?? Date()
		case "runActivity":
			guard let presentation = payload.presentation else {
				return
			}
			guard isStaleRunActivity(payload) == false else {
				return
			}

			operatorPresentation = presentation
			operatorSnapshotUpdatedAt = payload.emittedAt ?? Date()
		default:
			break
		}
	}

	private func isStaleRunActivity(_ payload: OperatorDashboardSocketPayload) -> Bool {
		guard let emittedAtUnixEpoch = payload.emittedAtUnixEpoch,
			let snapshotPublishedAtUnixEpoch = operatorSnapshotPublishedAtUnixEpoch
		else {
			return false
		}

		return emittedAtUnixEpoch < snapshotPublishedAtUnixEpoch
	}
}
