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
    /// Why the last account left this device, on the welcome screen: it
    /// was deleted (D74) or this device was signed out elsewhere.
    public private(set) var ended: String?

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
    /// ⌘, on macOS: each request shows Settings (D48 §2).
    public private(set) var settingsRequests = 0

    public func requestSettings() {
        settingsRequests += 1
    }

    /// macOS: "Sign Out…" in the menu bar opens Settings and asks the same
    /// question its Sign out button does.
    public private(set) var signOutAsked = false

    public func askSignOut() {
        requestSettings()
        signOutAsked = true
    }

    func signOutQuestionShown() {
        signOutAsked = false
    }

    private var account: (any Account)?
    private var accountServer: URL?
    private let makeAccount: @MainActor (URL) throws -> any Account
    private let defaults: UserDefaults
    private let pasteboard: any SecretPasteboard
    private let deviceName: String
    /// Whether PWM is the AutoFill provider (D47 §7).
    let autofill: any AutoFillSystem
    /// macOS: the browser extensions' Touch ID unlock (D48 §5); nil elsewhere.
    let browserTouchID: (any BrowserTouchID)?
    /// The notice for a new device's sign-in request (D50 §3).
    let approvalNotices: any ApprovalNotices
    /// Reading and deleting an import's file (D51 §9).
    let importFiles: any ImportFiles
    /// Where this app sells plans (D6 §2).
    let checkout: Checkout
    /// iOS: App Store transactions that finish outside a purchase, sent to
    /// the server while the vault is unlocked (D6 §3).
    private(set) var storeUpdates: Task<Void, Never>?
    /// "Sign in again to sync" (D50 §5), while the vault shows.
    public private(set) var signInAgain: SignInAgainModel?
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
        browserTouchID: (any BrowserTouchID)? = nil,
        approvalNotices: any ApprovalNotices = NoApprovalNotices(),
        importFiles: any ImportFiles = SystemImportFiles(),
        checkout: Checkout = .web,
        clock: @escaping @MainActor () -> ContinuousClock.Instant = { ContinuousClock.now }
    ) {
        self.makeAccount = makeAccount
        self.defaults = defaults
        self.pasteboard = pasteboard
        self.deviceName = deviceName
        self.autofill = autofill
        self.browserTouchID = browserTouchID
        self.approvalNotices = approvalNotices
        self.importFiles = importFiles
        self.checkout = checkout
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
                await sendPushToken()
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
        ended = nil
        createAccount = CreateAccountModel(deviceName: deviceName, account: { [unowned self] in try self.chosenAccount() }) {
            [unowned self] account, key in
            self.createAccount = nil
            self.secretKey = SecretKeyModel(secretKey: key, account: account, pasteboard: self.pasteboard) { [unowned self] in
                self.secretKey = nil
                self.showVault()
            }
            self.signedIn()
            self.screen = .secretKey
        }
        screen = .createAccount
    }

    public func showSignIn() {
        error = nil
        ended = nil
        signIn = SignInModel(
            deviceName: deviceName,
            account: { [unowned self] in try self.chosenAccount() },
            onScannedServer: { [unowned self] server in
                self.usesOtherServer = server != nil
                self.otherServer = server?.absoluteString ?? ""
            },
            onSignedIn: { [unowned self] in
                self.signIn = nil
                self.signedIn()
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
            self.didSignOut()
        }, beforeSignOut: { [unowned self] in
            await self.forgetPushToken()
        })
        screen = .unlock
    }

    private func showVault() {
        guard let account else { return }
        let vault = VaultModel(account: account, pasteboard: pasteboard) { [unowned self] ended in
            self.didSignOut(ended: ended)
        }
        // Signing in again already synced; the server needs the push token again.
        let again = SignInAgainModel(account: account, defaults: defaults) { [weak self, weak vault] in
            await vault?.load()
            self?.defaults.removeObject(forKey: AppModel.pushSentKey)
            await self?.sendPushToken()
        }
        vault.signInAgain = again
        self.vault = vault
        signInAgain = again
        generator = GeneratorModel(pasteboard: pasteboard)
        let server = accountServer.flatMap { $0 == Server.hosted ? nil : $0 }
        settings = SettingsModel(server: server, app: self, account: account)
        screen = .vault
        listenForStoreTransactions(account: account)
    }

    /// Unfinished and later App Store transactions go to the server, then
    /// are finished; the plan on screen updates. Only while unlocked: the
    /// server call needs the session, and StoreKit offers what's left again.
    private func listenForStoreTransactions(account: any Account) {
        guard case let .appStore(store) = checkout else { return }
        storeUpdates?.cancel()
        storeUpdates = Task { [weak self] in
            for await transaction in store.transactions() {
                guard let status = try? await PlanModel.deliver(transaction, to: account) else { continue }
                self?.settings?.plan.apply(status)
            }
        }
    }

    /// Back to welcome once the account is gone from this device; `ended`
    /// says why when it wasn't signed out here (`account_deleted`,
    /// `device_revoked`).
    func didSignOut(ended code: String? = nil) {
        ended = code.map { PWMError(code: $0).message }
        dropVault()
        unlock = nil
        SignInAgainModel.clear(in: defaults)
        defaults.removeObject(forKey: Self.pushSentKey)
        showWelcome()
    }

    private func dropVault() {
        storeUpdates?.cancel()
        storeUpdates = nil
        vault = nil
        generator = nil
        settings = nil
        signInAgain = nil
        backgroundedAt = nil
    }

    /// A new sign-in on this device: its session is new, and the server
    /// needs this device's push token again.
    private func signedIn() {
        SignInAgainModel.clear(in: defaults)
        defaults.removeObject(forKey: Self.pushSentKey)
        Task { await sendPushToken() }
    }

    // MARK: Background sync and push (D50)

    static let pushTokenKey = "pushToken"
    static let pushSandboxKey = "pushSandbox"
    /// The token the server last accepted.
    static let pushSentKey = "pushTokenSent"

    /// The system gave this device's APNs token (at every launch). It goes
    /// to the server when it changed, and after each sign-in.
    public func pushTokenReceived(_ token: Data, sandbox: Bool) async {
        defaults.set(token, forKey: Self.pushTokenKey)
        defaults.set(sandbox, forKey: Self.pushSandboxKey)
        await sendPushToken()
    }

    func sendPushToken() async {
        guard let account, let token = defaults.data(forKey: Self.pushTokenKey),
              defaults.data(forKey: Self.pushSentKey) != token,
              (try? await account.isSignedIn()) == true
        else { return }
        do {
            try await account.setPushToken(token, sandbox: defaults.bool(forKey: Self.pushSandboxKey))
            defaults.set(token, forKey: Self.pushSentKey)
        } catch {
            // Sent again at the next launch.
        }
    }

    /// Removes this device's push token from the server before signing out.
    func forgetPushToken() async {
        guard let account, defaults.data(forKey: Self.pushSentKey) != nil else { return }
        try? await account.setPushToken(nil, sandbox: defaults.bool(forKey: Self.pushSandboxKey))
        defaults.removeObject(forKey: Self.pushSentKey)
    }

    /// From background refresh or a push, locked or not (D50 §2). The vault
    /// and the approval count are read again if they show.
    public func backgroundSync() async -> BackgroundResult {
        guard let account, (try? await account.isSignedIn()) == true else { return .noData }
        do {
            switch try await account.backgroundSync() {
            case let .synced(pulled, pushed, _, _):
                await vault?.load()
                await settings?.checkApprovals()
                return pulled + pushed > 0 ? .newData : .noData
            case .retry:
                return .failed
            case .signedOut:
                if let signInAgain {
                    signInAgain.sessionEnded()
                } else {
                    SignInAgainModel.sessionEnded(in: defaults)
                }
                return .noData
            case .deviceRevoked:
                // The core has cleared the cache; finish signing out.
                try? await account.signOut()
                didSignOut(ended: "device_revoked")
                return .noData
            case .accountDeleted:
                // As for a revoked device (D74 §5).
                try? await account.signOut()
                didSignOut(ended: "account_deleted")
                return .noData
            }
        } catch {
            return .failed
        }
    }

    // MARK: Locking (D43 §5)

    /// The breach check runs in the health report; on by default (D22 §5).
    public var breachCheckOn: Bool {
        get { defaults.object(forKey: Self.breachCheckKey) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Self.breachCheckKey) }
    }

    static let breachCheckKey = "breachCheck"

    /// macOS: the window is left out of screenshots, recordings and screen
    /// sharing while it shows the vault or the Secret Key; on by default (D49 §5).
    public var hidesFromCapture: Bool {
        get { defaults.object(forKey: Self.hidesFromCaptureKey) as? Bool ?? true }
        set {
            defaults.set(newValue, forKey: Self.hidesFromCaptureKey)
            hidesFromCaptureChanged += 1
        }
    }

    static let hidesFromCaptureKey = "hideFromCapture"
    /// Read by the view so a change applies at once.
    private(set) var hidesFromCaptureChanged = 0

    /// Whether the window is left out of captures now (macOS).
    public var hidesWindow: Bool {
        _ = hidesFromCaptureChanged
        return hidesFromCapture && (screen == .vault || screen == .secretKey)
    }

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
