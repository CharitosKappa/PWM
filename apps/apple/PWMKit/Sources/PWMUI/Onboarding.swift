import Foundation
import Observation
import PWMKit

// The onboarding screens' state (D42 §4). The master password and Secret Key
// live in these models only while their screen shows (SwiftUI text fields
// need a binding), and are cleared as soon as they are sent to the core.

/// Create account: email, master password twice, the strength meter.
@MainActor
@Observable
public final class CreateAccountModel {
    /// The same bar as the extension: "strong" or better.
    public static let minimumScore = 3

    public var email = ""
    public var password = ""
    public var confirm = ""
    public private(set) var strength: Generator.Strength?
    public private(set) var busy = false
    public private(set) var error: String?

    private let deviceName: String
    private let account: @MainActor () throws -> any Account
    private let onCreated: @MainActor (any Account, Redacted<String>) -> Void

    init(
        deviceName: String,
        account: @escaping @MainActor () throws -> any Account,
        onCreated: @escaping @MainActor (any Account, Redacted<String>) -> Void
    ) {
        self.deviceName = deviceName
        self.account = account
        self.onCreated = onCreated
    }

    public var passwordsMatch: Bool { !confirm.isEmpty && password == confirm }
    public var strongEnough: Bool { (strength?.score ?? 0) >= Self.minimumScore }
    public var canSubmit: Bool {
        !busy && trimmedEmail.contains("@") && strongEnough && passwordsMatch
    }

    private var trimmedEmail: String { email.trimmingCharacters(in: .whitespacesAndNewlines) }

    /// Scores the current password off the main thread; call it whenever the
    /// password or email changes (a newer call makes an older one moot).
    public func updateStrength() async {
        let password = Redacted(password)
        let context = [trimmedEmail]
        guard !password.value.isEmpty else {
            strength = nil
            return
        }
        let scored = await Task.detached { Generator.strength(of: password, context: context) }.value
        if !Task.isCancelled, password.value == self.password {
            strength = scored
        }
    }

    public func submit() async {
        guard canSubmit else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            let account = try account()
            let key = try await account.signUp(email: trimmedEmail, password: Redacted(password), deviceName: deviceName)
            password = ""
            confirm = ""
            onCreated(account, key)
        } catch {
            self.error = message(for: error)
        }
    }
}

/// The Secret Key, shown once, and the Emergency Kit.
@MainActor
@Observable
public final class SecretKeyModel {
    public let secretKey: Redacted<String>
    /// The kit PDF, kept only while this screen shows.
    public private(set) var kit: Redacted<Data>?
    public private(set) var saved = false
    public private(set) var copied = false
    public private(set) var error: String?

    /// How long a copied Secret Key stays on the pasteboard, as in the extension.
    public static let clipboardSeconds: TimeInterval = 30

    private let account: any Account
    private let pasteboard: any SecretPasteboard
    private let capture: ScreenCapture
    private let onDone: @MainActor () -> Void

    init(
        secretKey: Redacted<String>, account: any Account, pasteboard: any SecretPasteboard,
        capture: ScreenCapture = .shared, onDone: @escaping @MainActor () -> Void
    ) {
        self.secretKey = secretKey
        self.account = account
        self.pasteboard = pasteboard
        self.capture = capture
        self.onDone = onDone
    }

    /// The key shows `ScreenCapture.hiddenLine` instead (D49); Copy and the
    /// kit still work.
    public var hiddenByCapture: Bool { capture.isRecorded }

    /// "I saved it" needs a saved or printed kit, or a copied key.
    public var canContinue: Bool { saved || copied }

    public func prepareKit(paper: EmergencyKit.Paper = .of(.current)) async {
        do {
            kit = try await account.emergencyKit(paper: paper)
        } catch {
            self.error = message(for: error)
        }
    }

    /// The share sheet was opened with the kit.
    public func didShareKit() {
        saved = true
    }

    public func copy() {
        pasteboard.copy(secretKey, clearAfter: Self.clipboardSeconds)
        copied = true
    }

    public func done() {
        guard canContinue else { return }
        kit = nil
        onDone()
    }
}

/// Sign in on a new device: email, master password and Secret Key, typed or
/// scanned from the kit.
@MainActor
@Observable
public final class SignInModel {
    public var email = ""
    public var password = ""
    public var secretKey = ""
    public private(set) var busy = false
    public private(set) var error: String?
    /// Asking a signed-in device (D46): the code to compare there.
    public private(set) var approvalCode: String?
    public private(set) var asking = false
    /// The Secret Key came from a signed-in device; only the master
    /// password is left to type.
    public private(set) var approved = false

    private let deviceName: String
    private var askTask: Task<Void, Never>?
    private let account: @MainActor () throws -> any Account
    private let onScannedServer: @MainActor (URL?) -> Void
    private let onSignedIn: @MainActor () -> Void

    init(
        deviceName: String,
        account: @escaping @MainActor () throws -> any Account,
        onScannedServer: @escaping @MainActor (URL?) -> Void,
        onSignedIn: @escaping @MainActor () -> Void
    ) {
        self.deviceName = deviceName
        self.account = account
        self.onScannedServer = onScannedServer
        self.onSignedIn = onSignedIn
    }

    private var trimmedEmail: String { email.trimmingCharacters(in: .whitespacesAndNewlines) }

    public var canSubmit: Bool {
        !busy && trimmedEmail.contains("@") && !password.isEmpty && !secretKey.isEmpty
    }

    public var canAsk: Bool { !busy && !asking && trimmedEmail.contains("@") }

    /// "Sign in with another device": waits for a signed-in device to
    /// approve, showing the code to compare, then fills in the Secret Key.
    public func askAnotherDevice() async {
        guard canAsk else { return }
        asking = true
        approvalCode = nil
        error = nil
        defer {
            asking = false
            approvalCode = nil
        }
        do {
            let key = try await account().askForApproval(email: trimmedEmail) { [weak self] code in
                await MainActor.run { self?.approvalCode = code }
            }
            secretKey = key.value
            approved = true
        } catch is CancellationError {
            return
        } catch {
            self.error = message(for: error)
        }
    }

    /// Starts asking in a task the screen can cancel.
    public func startAsking() {
        askTask?.cancel()
        askTask = Task { await askAnotherDevice() }
    }

    public func cancelAsking() {
        askTask?.cancel()
        askTask = nil
    }

    /// A scanned QR code: a kit fills in the email, Secret Key and server,
    /// never the password. Returns whether it was a kit.
    @discardableResult
    public func scanned(_ text: String) -> Bool {
        do {
            let kit = try EmergencyKit.scan(text)
            email = kit.email
            secretKey = kit.secretKey.value
            onScannedServer(kit.server)
            error = nil
            return true
        } catch {
            self.error = message(for: error)
            return false
        }
    }

    public func submit() async {
        guard canSubmit else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            _ = try await account().logIn(
                email: trimmedEmail, password: Redacted(password), secretKey: Redacted(secretKey), deviceName: deviceName
            )
            password = ""
            secretKey = ""
            onSignedIn()
        } catch {
            self.error = message(for: error)
        }
    }
}

/// Unlock on later launches: the master password only.
@MainActor
@Observable
public final class UnlockModel {
    public let email: String?
    public var password = ""
    public private(set) var busy = false
    public private(set) var error: String?
    /// Face ID / Touch ID for this account (D45); nil until loaded.
    public private(set) var biometric: BiometricState?
    /// After a master password unlock: "Use Face ID next time?", shown once.
    public private(set) var offer: BiometricKind?

    private let account: any Account
    private let defaults: UserDefaults
    private let onUnlocked: @MainActor () -> Void
    private let onSignedOut: @MainActor () -> Void
    /// Face ID changed: turn it back on after this password unlock (D45 §4).
    private var reenable = false

    static let offeredKey = "biometricOffered"
    static let unlockReason = "Unlock your vault"
    static let enableReason = "Turn on unlocking with your face or fingerprint"

    init(
        email: String?, account: any Account, defaults: UserDefaults = .standard,
        onUnlocked: @escaping @MainActor () -> Void, onSignedOut: @escaping @MainActor () -> Void
    ) {
        self.email = email
        self.account = account
        self.defaults = defaults
        self.onUnlocked = onUnlocked
        self.onSignedOut = onSignedOut
    }

    public var canSubmit: Bool { !busy && !password.isEmpty }

    /// Face ID or Touch ID can unlock now.
    public var biometricKind: BiometricKind? {
        if case let .on(kind) = biometric { kind } else { nil }
    }

    public func load() async {
        biometric = try? await account.biometricState()
    }

    public func submit() async {
        guard canSubmit else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            try await account.unlock(password: Redacted(password))
            password = ""
        } catch {
            password = ""
            self.error = message(for: error)
            return
        }
        if reenable {
            reenable = false
            try? await account.enableBiometricUnlock(reason: Self.enableReason)
        } else if case let .off(kind) = biometric, !defaults.bool(forKey: Self.offeredKey) {
            offer = kind
            return
        }
        onUnlocked()
    }

    /// One try with Face ID or Touch ID; the password field stays for a cancel.
    public func unlockWithBiometrics() async {
        guard biometricKind != nil, !busy else { return }
        busy = true
        error = nil
        defer { busy = false }
        do {
            try await account.unlockWithBiometrics(reason: Self.unlockReason)
            onUnlocked()
        } catch let failure as PWMError where failure.code == "biometric_cancelled" {
            return
        } catch let failure as PWMError where failure.code == "biometric_changed" {
            reenable = true
            biometric = try? await account.biometricState()
            self.error = failure.message
        } catch {
            self.error = message(for: error)
        }
    }

    /// The one-time offer, answered either way, is never shown again.
    public func acceptOffer() async {
        defaults.set(true, forKey: Self.offeredKey)
        do {
            try await account.enableBiometricUnlock(reason: Self.enableReason)
        } catch {
            // Left off; Settings can turn it on later.
        }
        offer = nil
        onUnlocked()
    }

    public func declineOffer() {
        defaults.set(true, forKey: Self.offeredKey)
        offer = nil
        onUnlocked()
    }

    public func signOut() async {
        do {
            try await account.signOut()
            password = ""
            onSignedOut()
        } catch {
            self.error = message(for: error)
        }
    }
}
