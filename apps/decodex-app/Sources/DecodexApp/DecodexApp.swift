import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
	func applicationDidFinishLaunching(_ notification: Notification) {
		NSApp.setActivationPolicy(.accessory)
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
	@StateObject private var store: AccountStore
	@StateObject private var loginWindowState = LoginWindowState()

	@MainActor
	init() {
		let accountStore = AccountStore()

		_store = StateObject(wrappedValue: accountStore)
		Task {
			await accountStore.refreshIfNeeded()
			accountStore.startAutomaticRefresh()
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
	}

	@ViewBuilder
	private var menuBarContent: some View {
		let content = AccountPanelView(store: store, loginWindowState: loginWindowState)
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
