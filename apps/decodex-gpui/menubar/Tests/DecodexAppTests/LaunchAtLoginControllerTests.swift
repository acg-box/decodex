import AppKit
@testable import DecodexApp
import XCTest

@MainActor
final class LaunchAtLoginControllerTests: XCTestCase {
	func testEnableRegistersTheMainAppAndReadsBackEnabled() {
		let service = FakeMainAppLoginItemService(state: .notRegistered)
		service.stateAfterRegister = .enabled
		let controller = LaunchAtLoginController(service: service)

		XCTAssertEqual(controller.setEnabled(true), .enabled)
		XCTAssertEqual(service.registerCount, 1)
		XCTAssertEqual(service.unregisterCount, 0)
	}

	func testEnablePreservesTheSystemApprovalState() {
		let service = FakeMainAppLoginItemService(state: .notRegistered)
		service.stateAfterRegister = .requiresApproval
		let controller = LaunchAtLoginController(service: service)

		XCTAssertEqual(controller.setEnabled(true), .requiresApproval)
		XCTAssertTrue(controller.state.isRequested)
	}

	func testDisableUnregistersAnApprovedOrEnabledRequest() {
		for initial in [LaunchAtLoginState.enabled, .requiresApproval] {
			let service = FakeMainAppLoginItemService(state: initial)
			service.stateAfterUnregister = .notRegistered
			let controller = LaunchAtLoginController(service: service)

			XCTAssertEqual(controller.setEnabled(false), .notRegistered)
			XCTAssertEqual(service.unregisterCount, 1)
		}
	}

	func testIdempotentRequestDoesNotReregisterOrUnregister() {
		let enabled = FakeMainAppLoginItemService(state: .enabled)
		let enabledController = LaunchAtLoginController(service: enabled)
		XCTAssertEqual(enabledController.setEnabled(true), .enabled)

		let disabled = FakeMainAppLoginItemService(state: .notRegistered)
		let disabledController = LaunchAtLoginController(service: disabled)
		XCTAssertEqual(disabledController.setEnabled(false), .notRegistered)

		XCTAssertEqual(enabled.registerCount + disabled.registerCount, 0)
		XCTAssertEqual(enabled.unregisterCount + disabled.unregisterCount, 0)
	}

	func testMutationFailureIsTypedWithoutInventingARegistrationState() {
		let service = FakeMainAppLoginItemService(state: .notRegistered)
		service.error = TestFailure.expected
		let controller = LaunchAtLoginController(service: service)

		XCTAssertEqual(controller.setEnabled(true), .operationFailed)
		XCTAssertEqual(controller.state, .notRegistered)
	}

	func testLoginLaunchDetectionUsesTheDocumentedOpenApplicationAttribute() {
		XCTAssertFalse(wasLaunchedAsLoginItem(event: nil))

		let event = NSAppleEventDescriptor(
			eventClass: AEEventClass(kCoreEventClass),
			eventID: AEEventID(kAEOpenApplication),
			targetDescriptor: nil,
			returnID: AEReturnID(kAutoGenerateReturnID),
			transactionID: AETransactionID(kAnyTransactionID)
		)
		event.setAttribute(
			NSAppleEventDescriptor(boolean: true),
			forKeyword: keyAELaunchedAsLogInItem
		)

		XCTAssertTrue(wasLaunchedAsLoginItem(event: event))
	}
}

@MainActor
private final class FakeMainAppLoginItemService: MainAppLoginItemService {
	var state: LaunchAtLoginState
	var stateAfterRegister: LaunchAtLoginState?
	var stateAfterUnregister: LaunchAtLoginState?
	var error: Error?
	var registerCount = 0
	var unregisterCount = 0

	init(state: LaunchAtLoginState) {
		self.state = state
	}

	func register() throws {
		registerCount += 1
		if let error {
			throw error
		}
		if let stateAfterRegister {
			state = stateAfterRegister
		}
	}

	func unregister() throws {
		unregisterCount += 1
		if let error {
			throw error
		}
		if let stateAfterUnregister {
			state = stateAfterUnregister
		}
	}
}

private enum TestFailure: Error {
	case expected
}
