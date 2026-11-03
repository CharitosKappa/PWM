import Foundation
import Observation
import PWMKit

/// The app's state (D42 §4, D43): welcome, create account, the Secret Key,
/// sign in, unlock, then the vault with its tabs. It also locks the vault
/// after the lock timeout (D43 §5).
@MainActor
@Observable
public final class AppModel {
    public enum Screen: Equatable, Sendable {
        case loading, welcome, createAccount, secretKey, signIn, unlock, vault
    }

    public private(set) var screen: Screen = .loading
    public private(set) var error: String?

    /// The server choice on the welcome screen: the hosted service, or a
    /// self-hosted address.
    public var usesOtherServer = false
    public var otherServer = ""

    public private(set) var createAccount: CreateAccountModel?
    public private(set) var secretKey: SecretKeyModel?
    public private(set) var signIn: SignInModel?
    public private(set) var unlock: UnlockModel?
    public private(set) var vault: VaultModel?
    public private(set) var generator: GeneratorModel?
    public private(set) var settings: SettingsModel?

    private var account: (any Account)?
    private var accountServer: URL?
    private let makeAccount: @MainActor (URL) throws -> any Account
    private let defaults: UserDefaults
    private let pasteboard: any SecretPasteboard
    private let deviceName: String
    /// Whether PWM is the AutoFill provider (D47 §7).
    let autofill: any AutoFillSystem
    private let clock: @MainActor () -> ContinuousClock.Instant
    private var backgroundedAt: ContinuousClock.Instant?

    static let serverKey = "server"

    /// - Parameters:
    ///   - makeAccount: opens the device's account for a server.
    ///   - defaults: keeps the server address (`-server <url>` at launch
    ///     overrides it) and the lock timeout.
    ///   - clock: for the lock timeout; it keeps counting while the device sleeps.
    public init(
        makeAccount: @escaping @MainActor (URL) throws -> any Account,
        defaults: UserDefaults = .standard,
        pasteboard: any SecretPasteboard,
        deviceName: String,
        autofill: any AutoFillSystem = NoAutoFillSystem(),
        clock: @escaping @MainActor () -> ContinuousClock.Instant = { ContinuousClock.now }
    ) {
        self.makeAccount = makeAccount
        self.defaults = defaults
        self.pasteboard = pasteboard
        self.deviceName = deviceName
        self.autofill = autofill
        self.clock = clock
    }

    /// Opens the account on the saved server: unlock if one is signed in.
    public func start() async {
        let saved = defaults.string(forKey: Self.serverKey).flatMap(URL.init(string:)) ?? Server.hosted
        usesOtherServer = saved != Server.hosted
        otherServer = usesOtherServer ? saved.absoluteString : ""
        do {
            let account = try open(saved)
            if try await account.isSignedIn() {
                await showUnlock()
            } else {
                screen = .welcome
            }
        } catch {
            self.error = message(for: error)
            screen = .welcome
        }
    }

    // MARK: Navigation

    public func showCreateAccount() {
        error = nil
        createAccount = CreateAccountModel(deviceName: deviceName, account: { [unowned self] in try self.chosenAccount() }) {
            [unowned self] account, key in
            self.createAccount = nil
            self.secretKey = SecretKeyModel(secretKey: key, account: account, pasteboard: self.pasteboard) { [unowned self] in
                self.secretKey = nil
                self.showVault()
            }
            self.screen = .secretKey
        }
        screen = .createAccount
    }

    public func showSignIn() {
        error = nil
        signIn = SignInModel(
            deviceName: deviceName,
            account: { [unowned self] in try self.chosenAccount() },
            onScannedServer: { [unowned self] server in
                self.usesOtherServer = server != nil
                self.otherServer = server?.absoluteString ?? ""
            },
            onSignedIn: { [unowned self] in
                self.signIn = nil
                self.showVault()
            }
        )
        screen = .signIn
    }

    public func showWelcome() {
        createAccount = nil
        signIn = nil
        screen = .welcome
    }

    private func showUnlock() async {
        guard let account else { return }
        let email = (try? await account.email()) ?? nil
        unlock = UnlockModel(email: email, account: account, defaults: defaults, onUnlocked: { [unowned self] in
            self.unlock = nil
            self.showVault()
        }, onSignedOut: { [unowned self] in
            self.unlock = nil
            self.showWelcome()
        })
        screen = .unlock
    }

    private func showVault() {
        guard let account else { return }
        vault = VaultModel(account: account, pasteboard: pasteboard) { [unowned self] in self.didSignOut() }
        generator = GeneratorModel(pasteboard: pasteboard)
        let server = accountServer.flatMap { $0 == Server.hosted ? nil : $0 }
        settings = SettingsModel(server: server, app: self, account: account)
        screen = .vault
    }

    /// Back to welcome once the account is gone from this device.
    func didSignOut() {
        dropVault()
        unlock = nil
        showWelcome()
    }

    private func dropVault() {
        vault = nil
        generator = nil
        settings = nil
        backgroundedAt = nil
    }

    // MARK: Locking (D43 §5)

    /// The breach check runs in the health report; on by default (D22 §5).
    public var breachCheckOn: Bool {
        get { defaults.object(forKey: Self.breachCheckKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Self.breachCheckKey) }
    }

    static let breachCheckKey = "breachCheck"

    public var lockTimeout: LockTimeout {
        get { LockTimeout.saved(in: defaults) }
        set { defaults.set(newValue.rawValue, forKey: LockTimeout.key) }
    }

    /// Locks the vault and shows unlock. Only the vault locks: the Secret
    /// Key screen is shown once and stays until "I saved it".
    public func lock() async {
        guard screen == .vault, let account else { return }
        await account.lock()
        dropVault()
        await showUnlock()
    }

    /// The app left the screen.
    public func didEnterBackground() async {
        guard screen == .vault else { return }
        if lockTimeout == .immediately {
            await lock()
        } else {
            backgroundedAt = clock()
        }
    }

    /// The app is coming back: lock first if it was away too long, then
    /// look for requests from new devices.
    public func willEnterForeground() async {
        if let since = backgroundedAt {
            backgroundedAt = nil
            if clock() - since >= lockTimeout.duration {
                await lock()
                return
            }
        }
        await settings?.checkApprovals()
    }

    /// The device is locking: the vault locks too, whatever the timeout.
    public func deviceWillLock() async {
        await lock()
    }

    // MARK: Server

    /// The account on the server chosen on the welcome screen.
    private func chosenAccount() throws -> any Account {
        try open(try chosenServer())
    }

    func chosenServer() throws -> URL {
        guard usesOtherServer else { return Server.hosted }
        return try Self.serverAddress(otherServer)
    }

    /// A self-hosted address: HTTPS, or plain HTTP to this machine only (as
    /// in the extension and on the kit).
    static func serverAddress(_ text: String) throws -> URL {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let withoutSlash = trimmed.hasSuffix("/") ? String(trimmed.dropLast()) : trimmed
        guard let url = URL(string: withoutSlash), let host = url.host, !host.isEmpty else {
            throw PWMError(code: "invalid_server")
        }
        switch url.scheme {
        case "https": return url
        case "http" where ["localhost", "127.0.0.1", "::1"].contains(host): return url
        default: throw PWMError(code: "invalid_server")
        }
    }

    private func open(_ server: URL) throws -> any Account {
        if let account, accountServer == server { return account }
        let account = try makeAccount(server)
        self.account = account
        accountServer = server
        defaults.set(server.absoluteString, forKey: Self.serverKey)
        return account
    }
}
