import Foundation
import Observation
import PWMKit

#if canImport(UserNotifications)
import UserNotifications
#endif

/// How a background sync went, for the system's background refresh and
/// push callbacks (D50 §2).
public enum BackgroundResult: Sendable, Equatable {
    case newData, noData, failed
}

/// "Sign in again to sync" (D50 §5): the server refused this device's
/// refresh token. It shows over the vault list until the master password
/// is entered, and is kept across launches, so a refusal seen by a
/// background sync shows at the next open.
@MainActor
@Observable
public final class SignInAgainModel {
    static let key = "signInAgain"

    public private(set) var needed: Bool
    public var password = ""
    public private(set) var working = false
    public private(set) var error: String?

    private let account: any Account
    private let defaults: UserDefaults
    private let onSignedIn: @MainActor () async -> Void

    init(account: any Account, defaults: UserDefaults, onSignedIn: @escaping @MainActor () async -> Void) {
        self.account = account
        self.defaults = defaults
        self.onSignedIn = onSignedIn
        needed = defaults.bool(forKey: Self.key)
    }

    /// A sync was told the session ended.
    func sessionEnded() {
        needed = true
        defaults.set(true, forKey: Self.key)
    }

    static func sessionEnded(in defaults: UserDefaults) {
        defaults.set(true, forKey: key)
    }

    static func clear(in defaults: UserDefaults) {
        defaults.removeObject(forKey: key)
    }

    public var canSignIn: Bool { !password.isEmpty && !working }

    /// Returns whether it worked; the sheet closes then.
    @discardableResult
    public func signIn() async -> Bool {
        guard canSignIn else { return false }
        working = true
        defer { working = false }
        error = nil
        do {
            _ = try await account.signInAgain(password: Redacted(password))
            password = ""
            needed = false
            Self.clear(in: defaults)
            await onSignedIn()
            return true
        } catch {
            self.error = message(for: error)
            return false
        }
    }
}

/// The notice for a new device's sign-in request (D50 §3). The app asks the
/// system for permission only when the user turns the switch on.
public protocol ApprovalNotices: Sendable {
    func isOn() async -> Bool
    /// Asks the system the first time; returns whether notices are on. False
    /// after an earlier refusal: then only the system's settings can change it.
    func turnOn() async -> Bool
}

/// Where the system has no notifications (Linux tests).
public struct NoApprovalNotices: ApprovalNotices {
    public init() {}
    public func isOn() async -> Bool { false }
    public func turnOn() async -> Bool { false }
}

/// This app's page in the system's settings, where notices turn off (the
/// app can't). A literal, so the AutoFill extension builds too.
public let notificationSettingsURL: URL? = {
    #if os(macOS)
    URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension")
    #else
    URL(string: "app-settings:")
    #endif
}()

#if canImport(UserNotifications) && (os(iOS) || os(macOS))
/// The system's notification permission.
public struct SystemApprovalNotices: ApprovalNotices {
    public init() {}

    public func isOn() async -> Bool {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        return [.authorized, .provisional].contains(settings.authorizationStatus)
    }

    public func turnOn() async -> Bool {
        let center = UNUserNotificationCenter.current()
        switch await center.notificationSettings().authorizationStatus {
        case .notDetermined:
            return (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
        case .authorized, .provisional:
            return true
        default:
            return false
        }
    }
}
#endif
