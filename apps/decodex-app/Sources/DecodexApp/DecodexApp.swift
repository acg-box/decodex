import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate {
	func applicationDidFinishLaunching(_ notification: Notification) {
		NSApp.setActivationPolicy(.accessory)
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

@main
struct DecodexApp: App {
	@NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
	@State private var store: ResetCardStore

	@MainActor
	init() {
		let store = ResetCardStore()

		_store = State(initialValue: store)
		store.start()
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
		AccountPanelView(store: store)
			.environment(\.colorScheme, .dark)
			.preferredColorScheme(.dark)
			.containerBackground(.clear, for: .window)
	}
}
