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
    /// The app's model over the real `Session`: the cache in Application
    /// Support and the keys in the app's own Keychain group. The App Group
    /// and shared Keychain group come with the AutoFill extension (M5.5).
    public static func live() -> AppModel {
        AppModel(
            makeAccount: { server in
                let support = try FileManager.default.url(
                    for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
                )
                return try Session(cachePath: CacheLocation.path(in: support), server: server, keys: KeychainKeyStore())
            },
            pasteboard: SystemPasteboard(),
            deviceName: currentDeviceName
        )
    }
}
#endif
