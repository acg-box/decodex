import Foundation

final class AccountRunContinuousScroller {
	private var frameAction: ((TimeInterval) -> Bool)?
	private var lastTickTime: TimeInterval?
	private var timer: Timer?
	private var timerTarget: AccountRunContinuousTimerTarget?

	deinit {
		stop()
	}

	func start(_ frameAction: @escaping (TimeInterval) -> Bool) {
		stop()
		self.frameAction = frameAction
		lastTickTime = ProcessInfo.processInfo.systemUptime

		let timerTarget = AccountRunContinuousTimerTarget(scroller: self)
		let timer = Timer(
			timeInterval: AccountRunStripLayout.continuousScrollTickInterval,
			target: timerTarget,
			selector: #selector(AccountRunContinuousTimerTarget.timerDidFire(_:)),
			userInfo: nil,
			repeats: true
		)
		self.timerTarget = timerTarget
		self.timer = timer
		RunLoop.main.add(timer, forMode: .common)
	}

	func stop() {
		timer?.invalidate()
		timer = nil
		timerTarget = nil
		frameAction = nil
		lastTickTime = nil
	}

	fileprivate func performFrame() {
		guard let frameAction else {
			return
		}

		let tickTime = ProcessInfo.processInfo.systemUptime
		let elapsedTime = lastTickTime.map { tickTime - $0 }
			?? AccountRunStripLayout.continuousScrollTickInterval
		lastTickTime = tickTime

		let boundedElapsedTime = min(
			max(elapsedTime, 0),
			AccountRunStripLayout.continuousScrollMaximumFrameInterval
		)
		if frameAction(boundedElapsedTime) == false {
			stop()
		}
	}
}

private final class AccountRunContinuousTimerTarget: NSObject {
	weak var scroller: AccountRunContinuousScroller?

	init(scroller: AccountRunContinuousScroller) {
		self.scroller = scroller
	}

	@objc func timerDidFire(_ timer: Timer) {
		scroller?.performFrame()
	}
}
