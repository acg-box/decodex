import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
	func applicationDidFinishLaunching(_ notification: Notification) {
		NSApp.setActivationPolicy(.accessory)
	}
}

@MainActor
final class AppAppearanceStore: ObservableObject {
	@Published private(set) var colorScheme = AppAppearanceStore.currentColorScheme()
	private var observation: NSKeyValueObservation?
	private var distributedNotificationTokens = [NSObjectProtocol]()

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
final class LoginWindowState: ObservableObject {
	@Published var mode = AccountLoginSheetMode.newAccount
	@Published var isPresented = false
}

@main
struct DecodexApp: App {
	@NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
	@StateObject private var appAppearance = AppAppearanceStore()
	@StateObject private var store: AccountStore
	@StateObject private var loginWindowState = LoginWindowState()

	@MainActor
	init() {
		let accountStore = AccountStore()

		_store = StateObject(wrappedValue: accountStore)
		Task {
			accountStore.startAutomaticRefresh()
			await accountStore.refreshIfNeeded()
		}
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
		let content = AccountPanelView(store: store, loginWindowState: loginWindowState)
			.environment(\.colorScheme, appAppearance.colorScheme)
			.preferredColorScheme(appAppearance.colorScheme)
			.task {
				await store.refreshIfNeeded()
				store.startOperatorSnapshotStream()
			}

		if #available(macOS 15.0, *) {
			content
				.containerBackground(.clear, for: .window)
		} else {
			content
		}
	}
}
