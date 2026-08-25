import AppKit
import ServiceManagement

enum LaunchAtLoginState: Int32, Equatable {
	case notRegistered = 0
	case enabled = 1
	case requiresApproval = 2
	case notFound = 3
	case operationFailed = 4

	var isRequested: Bool {
		self == .enabled || self == .requiresApproval
	}
}

@MainActor
protocol MainAppLoginItemService: AnyObject {
	var state: LaunchAtLoginState { get }

	func register() throws
	func unregister() throws
}

@MainActor
final class SystemMainAppLoginItemService: MainAppLoginItemService {
	private let service: SMAppService

	init(service: SMAppService = .mainApp) {
		self.service = service
	}

	var state: LaunchAtLoginState {
		switch service.status {
		case .notRegistered:
			.notRegistered
		case .enabled:
			.enabled
		case .requiresApproval:
			.requiresApproval
		case .notFound:
			.notFound
		@unknown default:
			.operationFailed
		}
	}

	func register() throws {
		try service.register()
	}

	func unregister() throws {
		try service.unregister()
	}
}

@MainActor
final class LaunchAtLoginController {
	private let service: any MainAppLoginItemService

	init(service: any MainAppLoginItemService = SystemMainAppLoginItemService()) {
		self.service = service
	}

	var state: LaunchAtLoginState {
		service.state
	}

	func setEnabled(_ enabled: Bool) -> LaunchAtLoginState {
		let current = service.state
		if current.isRequested == enabled {
			return current
		}

		do {
			if enabled {
				try service.register()
			} else {
				try service.unregister()
			}
		} catch {
			return .operationFailed
		}
		return service.state
	}

	func openSystemSettings() {
		SMAppService.openSystemSettingsLoginItems()
	}
}

func wasLaunchedAsLoginItem(event: NSAppleEventDescriptor?) -> Bool {
	event?.attributeDescriptor(forKeyword: keyAELaunchedAsLogInItem) != nil
}
