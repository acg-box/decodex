import AppKit
import Observation
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
	func applicationDidFinishLaunching(_ notification: Notification) {
		NSApp.setActivationPolicy(.accessory)
	}
}

@MainActor
@Observable
final class AppAppearanceStore {
	private(set) var colorScheme = AppAppearanceStore.currentColorScheme()
	@ObservationIgnored private var observation: NSKeyValueObservation?
	@ObservationIgnored private var distributedNotificationTokens = [NSObjectProtocol]()

	init() {
		refreshColorScheme()
		observation = NSApp.observe(\.effectiveAppearance, options: [.new]) { [weak self] _, _ in
			Task { @MainActor in
				self?.refreshColorScheme()
			}
		}
		distributedNotificationTokens.append(
			DistributedNotificationCenter.default().addObserver(
				forName: Notification.Name("AppleInterfaceThemeChangedNotification"),
				object: nil,
				queue: .main
			) { [weak self] _ in
				Task { @MainActor in
					self?.refreshColorScheme()
				}
			}
		)
	}

	private func refreshColorScheme() {
		colorScheme = Self.currentColorScheme()
	}

	private static func currentColorScheme() -> ColorScheme {
		let darkAppearances: [NSAppearance.Name] = [
			.darkAqua,
			.vibrantDark,
			.accessibilityHighContrastDarkAqua,
			.accessibilityHighContrastVibrantDark,
		]
		let lightAppearances: [NSAppearance.Name] = [
			.aqua,
			.vibrantLight,
			.accessibilityHighContrastAqua,
			.accessibilityHighContrastVibrantLight,
		]
		let match = NSApp.effectiveAppearance.bestMatch(from: darkAppearances + lightAppearances)

		return darkAppearances.contains { $0 == match } ? .dark : .light
	}
}

enum AppAssets {
	static let statusBarIcon: NSImage = {
		let image = NSImage(named: "StatusBarIcon")
			?? Bundle.main.url(forResource: "StatusBarIcon", withExtension: "png")
				.flatMap(NSImage.init(contentsOf:))
			?? NSImage(systemSymbolName: "person.2.circle", accessibilityDescription: "Decodex")
			?? NSImage()
		image.isTemplate = true
		image.size = NSSize(width: 22, height: 22)
		return image
	}()
}

@MainActor
@Observable
final class LoginWindowState {
	var mode = AccountLoginSheetMode.newAccount
	var isPresented = false
}

@main
struct DecodexApp: App {
	@NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
	@State private var appAppearance = AppAppearanceStore()
	@State private var store: AccountStore
	@State private var resetCardStore: ResetCardStore
	@State private var loginWindowState = LoginWindowState()

	@MainActor
	init() {
		let accountStore = AccountStore()
		let resetStore = ResetCardStore()

		_store = State(initialValue: accountStore)
		_resetCardStore = State(initialValue: resetStore)
		accountStore.start()
		resetStore.start()
	}

	var body: some Scene {
		MenuBarExtra {
			menuBarContent
		} label: {
			Label {
				Text("Decodex")
			} icon: {
				Image(nsImage: AppAssets.statusBarIcon)
			}
		}
		.menuBarExtraStyle(.window)
		.windowResizability(.contentSize)
	}

	@ViewBuilder
	private var menuBarContent: some View {
		AccountPanelView(
			store: store,
			resetCardStore: resetCardStore,
			loginWindowState: loginWindowState
		)
			.environment(\.colorScheme, appAppearance.colorScheme)
			.preferredColorScheme(appAppearance.colorScheme)
			.containerBackground(.clear, for: .window)
	}
}
