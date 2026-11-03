// What feeds `ScreenCapture` from the system, and the macOS window setting
// (D49). Kept out of the models so the rules stay testable on Linux.
#if canImport(SwiftUI)
import PWMKit
import SwiftUI

#if os(iOS)
import UIKit

/// Reports recording, mirroring and capture over a cable while they last
/// (`sceneCaptureState`, iOS 17), from the scene this view is in.
struct CaptureWatcher: UIViewRepresentable {
    func makeUIView(context: Context) -> WatcherView { WatcherView() }
    func updateUIView(_ view: WatcherView, context: Context) {}

    final class WatcherView: UIView {
        private var registration: (any UITraitChangeRegistration)?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            isUserInteractionEnabled = false
            guard registration == nil, let scene = window?.windowScene else { return }
            Self.report(scene)
            registration = scene.registerForTraitChanges([UITraitSceneCaptureState.self]) { (scene: UIWindowScene, _: UITraitCollection) in
                Self.report(scene)
            }
        }

        private static func report(_ scene: UIWindowScene) {
            ScreenCapture.shared.setRecorded(scene.traitCollection.sceneCaptureState == .active)
        }
    }
}
#endif

#if os(macOS)
import AppKit

/// Leaves the window out of screenshots, recordings and screen sharing
/// while `hidden` (D49 §5). Capture apps that ignore the system's setting
/// can still see it: best effort.
struct WindowSharing: NSViewRepresentable {
    let hidden: Bool

    func makeNSView(context: Context) -> NSView { NSView() }

    func updateNSView(_ view: NSView, context: Context) {
        let hidden = hidden
        // The window is set once the view is in it.
        Task { @MainActor in
            view.window?.sharingType = hidden ? .none : .readOnly
        }
    }
}
#endif
#endif
