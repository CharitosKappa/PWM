import Foundation
import Observation
import PWMKit

/// What the AutoFill extension needs from the account (D47 §3-6).
public protocol AutoFillAccount: AnyObject, Sendable {
    func isSignedIn() async throws -> Bool
    func biometricState() async throws -> BiometricState
    func unlockWithBiometrics(reason: String) async throws
    func lock() async
    func fillChoices(for services: [String]) async throws -> [FillChoice]
    func allFillChoices() async throws -> [FillChoice]
    func fillCredential(_ itemId: String, in vaultId: String) async throws -> FillCredential
    func currentCode(_ itemId: String, in vaultId: String) async throws -> (code: Redacted<String>, secondsLeft: UInt64)?
    func passkeyChoices(rpId: String, allowed: [Data]) async throws -> [PasskeyChoice]
    func registerPasskey(rpId: String, userName: String, userHandle: Data) async throws -> NewPasskey
    func signWithPasskey(_ choice: PasskeyChoice, rpId: String, clientDataHash: Data) async throws -> PasskeySignature
}

extension Session: AutoFillAccount {}

/// What iOS asked the extension for.
public enum AutoFillRequest: Sendable, Equatable {
    /// The list for a page or app, from the keyboard's key icon.
    case logins(services: [String])
    /// A password suggestion the user chose above the keyboard.
    case login(record: String)
    /// A sign-in with a passkey; `allowed` empty means any for `rpId`.
    case passkeys(rpId: String, clientDataHash: Data, allowed: [Data])
    /// A passkey suggestion the user chose above the keyboard.
    case passkey(record: String, credentialId: Data, rpId: String, clientDataHash: Data)
    /// A site or app asks to create a passkey.
    case newPasskey(rpId: String, userName: String, userHandle: Data, clientDataHash: Data)
}

/// What the extension hands back to iOS.
public enum AutoFillResult: Sendable, Equatable {
    case login(username: String, password: Redacted<String>)
    case passkey(PasskeySignature, rpId: String, clientDataHash: Data)
    case newPasskey(NewPasskey, rpId: String, clientDataHash: Data)
    case cancelled
    /// The suggestion's item is gone: iOS removes it.
    case notFound
}

/// The extension's screen (D47 §3-4). It unlocks with Face ID or Touch ID
/// only, never the master password; it fills only after a tap; and it
/// locks again once it answered, so no key outlives one request.
@MainActor
@Observable
public final class AutoFillModel {
    public enum Screen: Equatable, Sendable {
        case starting
        /// One line saying what to do in the app instead.
        case blocked(String)
        case logins
        case passkeys
        case newPasskey
    }

    public private(set) var screen: Screen = .starting
    public private(set) var error: String?
    public private(set) var matches: [FillChoice] = []
    public private(set) var passkeys: [PasskeyChoice] = []
    public var query = ""

    public let request: AutoFillRequest
    private let account: any AutoFillAccount
    private let pasteboard: any SecretPasteboard
    private let finish: @MainActor (AutoFillResult) -> Void
    private var everything: [FillChoice] = []
    private var finished = false

    static let reason = "Fill from PWM"
    /// One-time codes stay on the pasteboard this long (D42).
    static let codeSeconds: TimeInterval = 30

    public init(
        request: AutoFillRequest,
        account: any AutoFillAccount,
        pasteboard: any SecretPasteboard,
        finish: @escaping @MainActor (AutoFillResult) -> Void
    ) {
        self.request = request
        self.account = account
        self.pasteboard = pasteboard
        self.finish = finish
    }

    /// The other logins: all of them under a search, else those not matched.
    public var others: [FillChoice] {
        let text = query.trimmingCharacters(in: .whitespaces)
        if text.isEmpty {
            let matched = Set(matches.map(\.id))
            return everything.filter { !matched.contains($0.id) }
        }
        return everything.filter {
            $0.title.localizedCaseInsensitiveContains(text) || $0.username.localizedCaseInsensitiveContains(text)
        }
    }

    /// For a new passkey: the site and the account name.
    public var newPasskeySite: (rpId: String, userName: String)? {
        if case let .newPasskey(rpId, userName, _, _) = request { (rpId, userName) } else { nil }
    }

    /// Checks that Face ID can unlock, asks for it, then shows the screen
    /// for the request (or fills a chosen suggestion at once).
    public func start() async {
        guard (try? await account.isSignedIn()) == true else {
            screen = .blocked("Open PWM and sign in to fill here.")
            return
        }
        switch (try? await account.biometricState()) ?? .unavailable(reason: "") {
        case .on:
            break
        case let .off(kind):
            screen = .blocked(kind == .faceID ? "Turn on Face ID in PWM to fill here." : "Turn on Touch ID in PWM to fill here.")
            return
        case .unavailable:
            screen = .blocked("Turn on Face ID or Touch ID in PWM to fill here.")
            return
        }
        do {
            try await account.unlockWithBiometrics(reason: Self.reason)
        } catch let failure as PWMError {
            switch failure.code {
            case "biometric_cancelled":
                await end(.cancelled)
            case "master_password_due", "biometric_changed":
                screen = .blocked("Open PWM and enter your master password.")
            default:
                screen = .blocked(failure.message)
            }
            return
        } catch {
            screen = .blocked(message(for: error))
            return
        }
        await show()
    }

    private func show() async {
        do {
            switch request {
            case let .logins(services):
                matches = try await account.fillChoices(for: services)
                everything = try await account.allFillChoices()
                screen = .logins
            case let .login(record):
                guard let chosen = Suggestion.parse(record: record) else {
                    await end(.notFound)
                    return
                }
                await fill(vaultId: chosen.vaultId, itemId: chosen.itemId)
            case let .passkeys(rpId, _, allowed):
                passkeys = try await account.passkeyChoices(rpId: rpId, allowed: allowed)
                screen = .passkeys
            case let .passkey(record, credentialId, rpId, _):
                let choices = try await account.passkeyChoices(rpId: rpId, allowed: [credentialId])
                guard let chosen = Suggestion.parse(record: record),
                      let choice = choices.first(where: { $0.vaultId == chosen.vaultId && $0.itemId == chosen.itemId })
                else {
                    await end(.notFound)
                    return
                }
                await sign(choice)
            case .newPasskey:
                screen = .newPasskey
            }
        } catch {
            screen = .blocked(message(for: error))
        }
    }

    /// A tap on a login: fills it, and copies its one-time code.
    public func fill(_ choice: FillChoice) async {
        await fill(vaultId: choice.vaultId, itemId: choice.itemId)
    }

    private func fill(vaultId: String, itemId: String) async {
        error = nil
        do {
            let credential = try await account.fillCredential(itemId, in: vaultId)
            if credential.hasCode, let code = try? await account.currentCode(itemId, in: vaultId) {
                pasteboard.copy(code.code, clearAfter: Self.codeSeconds)
            }
            await end(.login(username: credential.username, password: credential.password))
        } catch let failure as PWMError where failure.code == "unknown_item" {
            await end(.notFound)
        } catch {
            self.error = message(for: error)
        }
    }

    /// A tap on a passkey.
    public func choose(_ choice: PasskeyChoice) async {
        await sign(choice)
    }

    private func sign(_ choice: PasskeyChoice) async {
        let rpId: String, hash: Data
        switch request {
        case let .passkeys(id, clientDataHash, _), let .passkey(_, _, id, clientDataHash):
            (rpId, hash) = (id, clientDataHash)
        default:
            return
        }
        error = nil
        do {
            let signed = try await account.signWithPasskey(choice, rpId: rpId, clientDataHash: hash)
            await end(.passkey(signed, rpId: rpId, clientDataHash: hash))
        } catch {
            self.error = message(for: error)
        }
    }

    /// "Save passkey": creates it in the vault and hands it to iOS.
    public func saveNewPasskey() async {
        guard case let .newPasskey(rpId, userName, userHandle, hash) = request else { return }
        error = nil
        do {
            let made = try await account.registerPasskey(rpId: rpId, userName: userName, userHandle: userHandle)
            await end(.newPasskey(made, rpId: rpId, clientDataHash: hash))
        } catch {
            self.error = message(for: error)
        }
    }

    public func cancel() async {
        await end(.cancelled)
    }

    private func end(_ result: AutoFillResult) async {
        guard !finished else { return }
        finished = true
        await account.lock()
        finish(result)
    }
}

/// Whether PWM is the AutoFill provider, and the way to turn it on
/// (D47 §7): `ASSettingsHelper` on iOS, nothing elsewhere.
public protocol AutoFillSystem: Sendable {
    func isProviderOn() async -> Bool
    func openSettings() async
}

/// The browsers' manifests for Touch ID unlock through the Mac app
/// (D48 §5): written when the user turns it on, removed when off.
public protocol BrowserTouchID: Sendable {
    var isOn: Bool { get }
    func turnOn() throws
    func turnOff()
}

#if os(macOS) && canImport(Security) && canImport(LocalAuthentication)
public struct SystemBrowserTouchID: BrowserTouchID {
    let hosts: BrowserHosts

    public init(bundle: Bundle = .main) {
        let helper = bundle.bundleURL
            .appendingPathComponent("Contents/Library/LoginItems/PWM Browser Helper.app/Contents/MacOS/PWM Browser Helper")
        hosts = BrowserHosts(home: BrowserHelperPlaces.realHome, helperPath: helper.path)
    }

    public var isOn: Bool { hosts.isInstalled }
    public func turnOn() throws { try hosts.install() }
    public func turnOff() { hosts.remove() }
}
#endif

/// For platforms without an AutoFill provider yet (and previews): no card.
public struct NoAutoFillSystem: AutoFillSystem {
    public init() {}
    public func isProviderOn() async -> Bool { true }
    public func openSettings() async {}
}

#if (os(iOS) || os(macOS)) && canImport(AuthenticationServices)
import AuthenticationServices

public struct SystemAutoFill: AutoFillSystem {
    public init() {}

    public func isProviderOn() async -> Bool {
        await ASCredentialIdentityStore.shared.state().isEnabled
    }

    public func openSettings() async {
        try? await ASSettingsHelper.openCredentialProviderAppSettings()
    }
}
#endif
