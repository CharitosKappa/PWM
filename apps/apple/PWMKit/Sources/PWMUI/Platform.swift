import Foundation
import PWMKit

#if canImport(UIKit)
import UIKit
import UniformTypeIdentifiers
#elseif canImport(AppKit)
import AppKit
#endif

/// Copies a secret so it never syncs to other devices and clears itself.
public protocol SecretPasteboard: Sendable {
    @MainActor func copy(_ value: Redacted<String>, clearAfter seconds: TimeInterval)
}

#if canImport(UIKit)
/// The general pasteboard, local only (no Universal Clipboard) and expiring.
public struct SystemPasteboard: SecretPasteboard {
    public init() {}

    @MainActor public func copy(_ value: Redacted<String>, clearAfter seconds: TimeInterval) {
        UIPasteboard.general.setItems(
            [[UTType.plainText.identifier: value.value]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(seconds)]
        )
    }
}
#elseif canImport(AppKit)
/// The general pasteboard, cleared after `seconds` unless something else was
/// copied since. Marked concealed so clipboard managers skip it.
public struct SystemPasteboard: SecretPasteboard {
    public init() {}

    @MainActor public func copy(_ value: Redacted<String>, clearAfter seconds: TimeInterval) {
        let board = NSPasteboard.general
        board.clearContents()
        board.setString(value.value, forType: .string)
        board.setString("", forType: NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType"))
        let count = board.changeCount
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(seconds))
            if board.changeCount == count { board.clearContents() }
        }
    }
}
#endif

/// Says a short line to VoiceOver without moving focus: copies and locking
/// (design/README.md, Accessibility). Never a secret.
public protocol Announcer: Sendable {
    @MainActor func say(_ line: String)
}

public struct SystemAnnouncer: Announcer {
    public init() {}

    @MainActor public func say(_ line: String) {
        #if canImport(UIKit)
        // Queued, so it follows what VoiceOver is saying instead of cutting it off.
        let queued = NSAttributedString(string: line, attributes: [.accessibilitySpeechQueueAnnouncement: true])
        UIAccessibility.post(notification: .announcement, argument: queued)
        #elseif canImport(AppKit)
        guard let app = NSApp else { return }
        NSAccessibility.post(
            element: app, notification: .announcementRequested,
            userInfo: [.announcement: line, .priority: NSAccessibilityPriorityLevel.high.rawValue]
        )
        #endif
    }
}

/// Where the models' announcements go; tests swap in their own.
@MainActor
public enum Speech {
    public static var announcer: any Announcer = SystemAnnouncer()

    public static func say(_ line: String) {
        announcer.say(line)
    }
}

/// The name a new device sends at sign-in. iOS gives apps only the model
/// ("iPhone", "iPad") without an entitlement; rename comes in M5.3.
@MainActor
public var currentDeviceName: String {
    #if os(iOS)
    UIDevice.current.model
    #elseif os(macOS)
    "Mac"
    #else
    "Device"
    #endif
}

#if canImport(Security)
extension AppModel {
    /// The app's model over the real `Session`, in the places it shares
    /// with the AutoFill extension (D47 §2): the first launch moves the
    /// cache and keys there. On iOS it keeps the suggestions above the
    /// keyboard current and knows whether PWM is the AutoFill provider
    /// (on macOS too, D48 §2). `checkout`: the App Store on iOS, where the
    /// app target has StoreKit; the web on the Mac (D6 §2).
    public static func live(checkout: Checkout) -> AppModel {
        AppModel(
            makeAccount: { server in
                let places = try SharedStorage.app()
                return try Session(
                    cachePath: places.cachePath, server: server, keys: places.keys,
                    biometrics: places.biometrics, suggestions: liveSuggestions(shared: places.shared)
                )
            },
            pasteboard: SystemPasteboard(),
            deviceName: currentDeviceName,
            autofill: liveAutoFill,
            browserTouchID: liveBrowserTouchID,
            approvalNotices: liveApprovalNotices,
            checkout: checkout
        )
    }

    static var liveApprovalNotices: any ApprovalNotices {
        #if canImport(UserNotifications) && (os(iOS) || os(macOS))
        SystemApprovalNotices()
        #else
        NoApprovalNotices()
        #endif
    }

    static var liveBrowserTouchID: (any BrowserTouchID)? {
        #if os(macOS) && canImport(LocalAuthentication)
        SystemBrowserTouchID()
        #else
        nil
        #endif
    }

    static func liveSuggestions(shared: Bool) -> Suggestions? {
        #if os(iOS) || os(macOS)
        shared ? Suggestions(store: SystemSuggestionStore(), defaults: SharedStorage.defaults) : nil
        #else
        nil
        #endif
    }

    static var liveAutoFill: any AutoFillSystem {
        #if os(iOS) || os(macOS)
        SystemAutoFill()
        #else
        NoAutoFillSystem()
        #endif
    }
}
#endif
