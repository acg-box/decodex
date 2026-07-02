import SwiftUI

enum PanelMotion {
	static let hover = Animation.interactiveSpring(response: 0.22, dampingFraction: 0.86, blendDuration: 0.04)
	static let press = Animation.interactiveSpring(response: 0.16, dampingFraction: 0.78, blendDuration: 0.02)
	static let state = Animation.interactiveSpring(response: 0.24, dampingFraction: 0.88, blendDuration: 0.05)
	static let inlineLayout = Animation.interactiveSpring(response: 0.2, dampingFraction: 0.9, blendDuration: 0.03)
	static let panelLayout = Animation.interactiveSpring(response: 0.3, dampingFraction: 0.92, blendDuration: 0.05)
	static let accountRemoval = Animation.interactiveSpring(response: 0.28, dampingFraction: 0.94, blendDuration: 0.04)
}
