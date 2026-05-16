import SwiftUI

struct AccountPanelView: View {
	@ObservedObject var store: AccountStore
	@State private var pendingLogout: CodexAccount?
	@State private var loginPresented = false

	var body: some View {
		VStack(alignment: .leading, spacing: 14) {
			header

			if let notice = store.notice {
				NoticeView(text: notice)
			}

			if store.accounts.isEmpty {
				emptyState
			} else {
				accountList
			}

			Divider()

			footer
		}
		.frame(width: 420)
		.padding(16)
		.background(.regularMaterial)
		.confirmationDialog(
			"Remove account?",
			isPresented: Binding(
				get: { pendingLogout != nil },
				set: { visible in
					if !visible {
						pendingLogout = nil
					}
				}
			),
			titleVisibility: .visible
		) {
			if let account = pendingLogout {
				Button("Log Out \(account.displayName)", role: .destructive) {
					Task {
						await store.logout(account)
					}
				}
			}
		} message: {
			if let account = pendingLogout {
				Text("This removes \(account.displayName) from the Decodex account pool on this Mac.")
			}
		}
		.sheet(isPresented: $loginPresented) {
			LoginSheetView(store: store)
		}
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 12) {
			ZStack {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.fill(.quaternary)
				Image(systemName: "person.2.circle.fill")
					.font(.system(size: 24, weight: .semibold))
					.foregroundStyle(.primary)
			}
			.frame(width: 42, height: 42)

			VStack(alignment: .leading, spacing: 3) {
				Text("Decodex Accounts")
					.font(.headline)
				Text(store.modeLabel)
					.font(.caption)
					.foregroundStyle(.secondary)
					.lineLimit(1)
			}

			Spacer()

			Button {
				Task {
					await store.refresh()
				}
			} label: {
				Image(systemName: store.isRefreshing ? "arrow.triangle.2.circlepath.circle" : "arrow.clockwise")
			}
			.buttonStyle(.borderless)
			.help("Refresh")
			.disabled(store.isRefreshing)
		}
	}

	private var emptyState: some View {
		VStack(alignment: .leading, spacing: 10) {
			Image(systemName: "person.crop.circle.badge.plus")
				.font(.system(size: 28))
				.foregroundStyle(.secondary)
			Text("No accounts in the local pool")
				.font(.subheadline.weight(.semibold))
			Text("Add a ChatGPT login to use Decodex account selection for future runs.")
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(14)
		.background(.thinMaterial, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
	}

	private var accountList: some View {
		ScrollView {
			LazyVStack(spacing: 8) {
				ForEach(store.accounts) { account in
					AccountRowView(
						account: account,
						select: {
							Task {
								await store.select(account)
							}
						},
						logout: {
							pendingLogout = account
						}
					)
				}
			}
			.padding(.vertical, 1)
		}
		.frame(maxHeight: 340)
	}

	private var footer: some View {
		HStack(spacing: 10) {
			Button {
				loginPresented = true
			} label: {
				Label("Add Login", systemImage: "plus.circle")
			}

			Button {
				Task {
					await store.clearSelection()
				}
			} label: {
				Label("Balanced", systemImage: "arrow.triangle.branch")
			}
			.disabled(store.accounts.isEmpty)

			Spacer()

			SettingsLink {
				Image(systemName: "gearshape")
			}
			.help("Settings")
		}
	}
}

struct AccountRowView: View {
	let account: CodexAccount
	let select: () -> Void
	let logout: () -> Void

	var body: some View {
		HStack(spacing: 12) {
			statusStripe

			Button(action: select) {
				HStack(spacing: 10) {
					Image(systemName: account.selected ? "checkmark.circle.fill" : "circle")
						.foregroundStyle(account.selected ? .green : .secondary)
						.frame(width: 18)

					VStack(alignment: .leading, spacing: 4) {
						Text(account.displayName)
							.font(.subheadline.weight(.semibold))
							.lineLimit(1)
						HStack(spacing: 7) {
							Text(account.accountFingerprint)
							Text(account.statusLabel)
						}
						.font(.caption)
						.foregroundStyle(.secondary)
					}

					Spacer()
				}
				.contentShape(Rectangle())
			}
			.buttonStyle(.plain)

			Button(role: .destructive, action: logout) {
				Image(systemName: "rectangle.portrait.and.arrow.right")
			}
			.buttonStyle(.borderless)
			.help("Log out")
		}
		.padding(.vertical, 10)
		.padding(.horizontal, 10)
		.background(rowBackground, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
	}

	private var statusStripe: some View {
		RoundedRectangle(cornerRadius: 2)
			.fill(statusColor)
			.frame(width: 4, height: 42)
	}

	private var rowBackground: some ShapeStyle {
		account.selected ?
			AnyShapeStyle(Color(nsColor: .selectedContentBackgroundColor).opacity(0.28)) :
			AnyShapeStyle(.thinMaterial)
	}

	private var statusColor: Color {
		switch account.statusTone {
		case .ready: return .green
		case .selected: return .accentColor
		case .warning: return .yellow
		case .danger: return .red
		case .neutral: return .secondary
		}
	}
}

struct NoticeView: View {
	let text: String

	var body: some View {
		HStack(alignment: .top, spacing: 8) {
			Image(systemName: "exclamationmark.triangle")
				.foregroundStyle(.yellow)
			Text(text)
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.padding(10)
		.background(.thinMaterial, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
	}
}
