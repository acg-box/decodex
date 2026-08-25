import AppKit
import SwiftUI

@MainActor
final class DecodexMenuBarHost: NSObject {
	private let store: ResetCardStore
	private let launchAtLogin: LaunchAtLoginController
	private var statusPanelController: StatusPanelController?
	private var isShuttingDown = false

	override init() {
		ProcessInfo.processInfo.automaticTerminationSupportEnabled = false
		ProcessInfo.processInfo.disableAutomaticTermination(
			"Decodex owns a persistent menu bar status item."
		)
		ProcessInfo.processInfo.disableSuddenTermination()
		store = ResetCardStore()
		launchAtLogin = LaunchAtLoginController()
		super.init()
		store.start()
	}

	@discardableResult
	func setVisible(_ visible: Bool) -> Bool {
		guard isShuttingDown == false else {
			return false
		}
		if visible {
			if statusPanelController == nil {
				statusPanelController = StatusPanelController(store: store)
			}
		} else {
			statusPanelController?.invalidate()
			statusPanelController = nil
		}
		return statusPanelController != nil
	}

	func launchAtLoginState() -> LaunchAtLoginState {
		launchAtLogin.state
	}

	func setLaunchAtLogin(_ enabled: Bool) -> LaunchAtLoginState {
		launchAtLogin.setEnabled(enabled)
	}

	func openLoginItemsSettings() {
		launchAtLogin.openSystemSettings()
	}

	func prepareForTermination(_ completion: @escaping @MainActor () -> Void) {
		guard isShuttingDown == false else {
			completion()
			return
		}
		isShuttingDown = true
		statusPanelController?.invalidate()
		statusPanelController = nil
		Task { [store] in
			await store.prepareForApplicationTermination()
			await DecodexNativeClient.shutdownSharedSession()
			completion()
		}
	}
}

enum AppAssets {
	static let statusBarIcon: NSImage = {
		let image = Bundle.main.url(forResource: "StatusBarIcon", withExtension: "png")
			.flatMap(NSImage.init(contentsOf:))
			?? NSImage(named: "StatusBarIcon")
			?? NSImage(systemSymbolName: "person.2.circle", accessibilityDescription: "Decodex")
			?? NSImage()
		image.isTemplate = true
		image.size = NSSize(width: 22, height: 22)
		return image
	}()
}

private let menuBarABIVersion: UInt32 = 1
private let menuBarArtifactCohort: UInt32 = 6

@_cdecl("decodex_menu_bar_abi_version")
public func decodexMenuBarABIVersion() -> UInt32 {
	menuBarABIVersion
}

@_cdecl("decodex_menu_bar_artifact_cohort")
public func decodexMenuBarArtifactCohort() -> UInt32 {
	menuBarArtifactCohort
}

@_cdecl("decodex_menu_bar_create")
public func decodexMenuBarCreate() -> UnsafeMutableRawPointer? {
	guard Thread.isMainThread else {
		return nil
	}
	let address: UInt = MainActor.assumeIsolated {
		UInt(bitPattern: Unmanaged.passRetained(DecodexMenuBarHost()).toOpaque())
	}
	return UnsafeMutableRawPointer(bitPattern: address)
}

@_cdecl("decodex_menu_bar_set_visible")
public func decodexMenuBarSetVisible(
	_ opaqueHost: UnsafeMutableRawPointer?,
	_ visible: Bool
) -> Bool {
	guard Thread.isMainThread, let opaqueHost else {
		return false
	}
	let address = UInt(bitPattern: opaqueHost)
	return MainActor.assumeIsolated {
		guard let isolatedHost = UnsafeMutableRawPointer(bitPattern: address) else {
			return false
		}
		return Unmanaged<DecodexMenuBarHost>
			.fromOpaque(isolatedHost)
			.takeUnretainedValue()
			.setVisible(visible)
	}
}

@_cdecl("decodex_menu_bar_launch_at_login_status")
public func decodexMenuBarLaunchAtLoginStatus(
	_ opaqueHost: UnsafeMutableRawPointer?
) -> Int32 {
	guard Thread.isMainThread, let opaqueHost else {
		return LaunchAtLoginState.operationFailed.rawValue
	}
	let address = UInt(bitPattern: opaqueHost)
	return MainActor.assumeIsolated {
		guard let isolatedHost = UnsafeMutableRawPointer(bitPattern: address) else {
			return LaunchAtLoginState.operationFailed.rawValue
		}
		return Unmanaged<DecodexMenuBarHost>
			.fromOpaque(isolatedHost)
			.takeUnretainedValue()
			.launchAtLoginState()
			.rawValue
	}
}

@_cdecl("decodex_menu_bar_set_launch_at_login")
public func decodexMenuBarSetLaunchAtLogin(
	_ opaqueHost: UnsafeMutableRawPointer?,
	_ enabled: Bool
) -> Int32 {
	guard Thread.isMainThread, let opaqueHost else {
		return LaunchAtLoginState.operationFailed.rawValue
	}
	let address = UInt(bitPattern: opaqueHost)
	return MainActor.assumeIsolated {
		guard let isolatedHost = UnsafeMutableRawPointer(bitPattern: address) else {
			return LaunchAtLoginState.operationFailed.rawValue
		}
		return Unmanaged<DecodexMenuBarHost>
			.fromOpaque(isolatedHost)
			.takeUnretainedValue()
			.setLaunchAtLogin(enabled)
			.rawValue
	}
}

@_cdecl("decodex_menu_bar_open_login_items_settings")
public func decodexMenuBarOpenLoginItemsSettings(
	_ opaqueHost: UnsafeMutableRawPointer?
) -> Bool {
	guard Thread.isMainThread, let opaqueHost else {
		return false
	}
	let address = UInt(bitPattern: opaqueHost)
	return MainActor.assumeIsolated {
		guard let isolatedHost = UnsafeMutableRawPointer(bitPattern: address) else {
			return false
		}
		Unmanaged<DecodexMenuBarHost>
			.fromOpaque(isolatedHost)
			.takeUnretainedValue()
			.openLoginItemsSettings()
		return true
	}
}

@_cdecl("decodex_app_was_launched_as_login_item")
public func decodexAppWasLaunchedAsLoginItem() -> Bool {
	guard Thread.isMainThread else {
		return false
	}
	return MainActor.assumeIsolated {
		wasLaunchedAsLoginItem(event: NSAppleEventManager.shared().currentAppleEvent)
	}
}

@_cdecl("decodex_menu_bar_destroy")
public func decodexMenuBarDestroy(_ opaqueHost: UnsafeMutableRawPointer?) {
	guard Thread.isMainThread, let opaqueHost else {
		return
	}
	let address = UInt(bitPattern: opaqueHost)
	MainActor.assumeIsolated {
		guard let isolatedHost = UnsafeMutableRawPointer(bitPattern: address) else {
			return
		}
		let host = Unmanaged<DecodexMenuBarHost>.fromOpaque(isolatedHost)
		host.takeUnretainedValue().prepareForTermination {
			host.release()
		}
	}
}
