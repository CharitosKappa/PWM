import Foundation
import Observation

/// Whether the screen is being recorded, mirrored or captured, and when a
/// screenshot was taken (D49). The system feeds `shared` (`RootView` on
/// iOS); tests make their own. The models read it to decide what to hide.
@MainActor
@Observable
public final class ScreenCapture {
    public static let shared = ScreenCapture()

    public private(set) var isRecorded = false

    /// What every hidden secret shows instead while the screen is captured.
    public static let hiddenLine = "Hidden while your screen is recorded or shared."
    /// Under a value masked again after a screenshot.
    public static let screenshotLine = "Screenshots go to your photo library, so it's hidden again."

    @ObservationIgnored private var listeners: [WeakListener] = []

    public init() {}

    public func setRecorded(_ on: Bool) {
        guard on != isRecorded else { return }
        isRecorded = on
        if on { notify { $0.captureStarted() } }
    }

    public func screenshotTaken() {
        notify { $0.screenshotTaken() }
    }

    func add(_ listener: any CaptureListener) {
        listeners.removeAll { $0.value == nil }
        listeners.append(WeakListener(value: listener))
    }

    private func notify(_ event: (any CaptureListener) -> Void) {
        listeners.removeAll { $0.value == nil }
        for listener in listeners.compactMap(\.value) { event(listener) }
    }
}

/// A model that masks what it revealed when capture starts or a
/// screenshot is taken.
@MainActor
protocol CaptureListener: AnyObject {
    func captureStarted()
    func screenshotTaken()
}

extension CaptureListener {
    func screenshotTaken() {}
}

private struct WeakListener {
    weak var value: (any CaptureListener)?
}
