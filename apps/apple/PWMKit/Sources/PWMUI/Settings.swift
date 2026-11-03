import Foundation
import Observation
import PWMKit

/// The generator tab and sheet (D43 §2): the core's generator with the
/// extension's options. Values stay in memory and are never stored.
@MainActor
@Observable
public final class GeneratorModel {
    public enum Mode: String, CaseIterable, Sendable {
        case password, passphrase
    }

    public static let lengths = 8...64
    public static let wordCounts = 3...10
    public static let separators = ["-", ".", "_", " ", ","]

    public var mode: Mode = .password
    public var length = 20
    public var lowercase = true
    public var uppercase = true
    public var digits = true
    public var symbols = true
    public var avoidLookalikes = false
    public var words = 5
    public var separator = "-"
    public var capitalize = false
    public var addDigit = false

    public private(set) var value: Redacted<String>?
    public private(set) var strength: Generator.Strength?
    public private(set) var copied = false
    public private(set) var error: String?

    private let pasteboard: any SecretPasteboard

    init(pasteboard: any SecretPasteboard) {
        self.pasteboard = pasteboard
    }

    /// Changes whenever an option does, for the screen to regenerate.
    public var options: String {
        switch mode {
        case .password: "p\(length)\(lowercase)\(uppercase)\(digits)\(symbols)\(avoidLookalikes)"
        case .passphrase: "w\(words)\(separator)\(capitalize)\(addDigit)"
        }
    }

    /// A new value for the current options; the screen calls it when an
    /// option changes.
    public func regenerate() async {
        copied = false
        do {
            let generated: Generator.Generated
            switch mode {
            case .password:
                guard lowercase || uppercase || digits || symbols else { throw PWMError(code: "no_characters") }
                generated = try Generator.password(
                    length: length, lowercase: lowercase, uppercase: uppercase,
                    digits: digits, symbols: symbols, avoidLookalikes: avoidLookalikes
                )
            case .passphrase:
                generated = try Generator.passphrase(
                    words: words, separator: separator, capitalize: capitalize, addDigit: addDigit
                )
            }
            value = generated.value
            error = nil
            let scored = await Task.detached { Generator.strength(of: generated.value) }.value
            if value == generated.value { strength = scored }
        } catch {
            value = nil
            strength = nil
            self.error = message(for: error)
        }
    }

    public func copy() {
        guard let value else { return }
        pasteboard.copy(value, clearAfter: ItemDetailModel.clipboardSeconds)
        copied = true
    }
}

/// How long the vault stays unlocked while the app is in the background
/// (D43 §5). Locking the device always locks the vault.
public enum LockTimeout: Int, CaseIterable, Sendable {
    case immediately = 0
    case oneMinute = 1
    case fiveMinutes = 5
    case fifteenMinutes = 15
    case oneHour = 60
    case eightHours = 480

    public static let standard = LockTimeout.fifteenMinutes
    static let key = "lockTimeoutMinutes"

    public var duration: Duration { .seconds(rawValue * 60) }

    /// The saved choice, or the default.
    static func saved(in defaults: UserDefaults) -> LockTimeout {
        guard defaults.object(forKey: key) != nil else { return .standard }
        return LockTimeout(rawValue: defaults.integer(forKey: key)) ?? .standard
    }
}

/// The settings tab (M5.3 part; devices, health and export come in M5.10).
@MainActor
@Observable
public final class SettingsModel {
    public private(set) var email: String?
    /// The self-hosted address, or nil for the hosted service.
    public let server: URL?
    public private(set) var kit: Redacted<Data>?
    public private(set) var error: String?

    private unowned let app: AppModel
    private let account: any Account

    public private(set) var lockTimeout: LockTimeout

    init(server: URL?, app: AppModel, account: any Account) {
        self.server = server
        self.app = app
        self.account = account
        lockTimeout = app.lockTimeout
        breachCheck = app.breachCheckOn
    }

    /// Face ID / Touch ID (D45); nil until loaded.
    public private(set) var biometric: BiometricState?

    /// Whether PWM fills in other apps and Safari (D47 §7); nil until
    /// loaded. The card that explains how to turn it on shows while false.
    public private(set) var autofillOn: Bool?
    /// "Suggest logins above the keyboard" (D47 §5).
    public private(set) var suggestLogins = true

    public func load() async {
        email = (try? await account.email()) ?? nil
        biometric = try? await account.biometricState()
        suggestLogins = await account.suggestionsOn()
        await loadAutoFill()
    }

    /// Read again when the app comes back from the Settings app.
    public func loadAutoFill() async {
        let was = autofillOn
        autofillOn = await app.autofill.isProviderOn()
        // iOS takes the suggestions only once PWM is the provider.
        if was == false, autofillOn == true {
            await account.setSuggestions(on: suggestLogins)
        }
    }

    public func openAutoFillSettings() async {
        await app.autofill.openSettings()
    }

    /// Off empties the system's list; on sends it again.
    public func setSuggestLogins(_ on: Bool) async {
        await account.setSuggestions(on: on)
        suggestLogins = on
    }

    /// Turning it on asks for Face ID once; off deletes the keys.
    public func setBiometric(_ on: Bool) async {
        error = nil
        do {
            if on {
                try await account.enableBiometricUnlock(reason: UnlockModel.enableReason)
            } else {
                try await account.disableBiometricUnlock()
            }
        } catch let failure as PWMError where failure.code == "biometric_cancelled" {
            // Left as it was.
        } catch {
            self.error = message(for: error)
        }
        biometric = try? await account.biometricState()
    }

    public func setLockTimeout(_ timeout: LockTimeout) {
        app.lockTimeout = timeout
        lockTimeout = timeout
    }

    /// The leak check in the health report (D22 §5).
    public var breachCheckOn: Bool { breachCheck }
    private var breachCheck = true

    public func setBreachCheck(_ on: Bool) {
        app.breachCheckOn = on
        breachCheck = on
    }

    public func makeHealth() -> HealthModel? {
        app.vault.map { HealthModel(vault: $0, breachCheckOn: breachCheck) }
    }

    public func makeDevices() -> DevicesModel {
        DevicesModel(account: account)
    }

    public func makeExport() -> ExportModel {
        ExportModel(account: account)
    }

    public func makeApprovals() -> ApprovalsModel {
        ApprovalsModel(account: account)
    }

    /// Requests from new devices, read when the app comes to the front (D46 §5).
    public private(set) var pendingApprovals = 0

    public func checkApprovals() async {
        pendingApprovals = (try? await account.approvalRequests().count) ?? 0
    }

    /// The app's version, from its Info.plist.
    public var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "–"
    }

    /// The kit PDF for "Save Emergency Kit"; dropped when the screen goes.
    public func prepareKit(paper: EmergencyKit.Paper = .of(.current)) async {
        do {
            kit = try await account.emergencyKit(paper: paper)
        } catch {
            self.error = message(for: error)
        }
    }

    public func dropKit() {
        kit = nil
    }

    public func lockNow() async {
        await app.lock()
    }

    /// The screen asks first: changes not yet synced are lost.
    public func signOut() async {
        do {
            try await account.signOut()
            app.didSignOut()
        } catch {
            self.error = message(for: error)
        }
    }
}
