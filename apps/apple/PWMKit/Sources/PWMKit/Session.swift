import Foundation
@preconcurrency import PWMCore

/// How a sync run ended.
public enum SyncResult: Sendable, Equatable {
    case synced(pulled: UInt64, pushed: UInt64, conflicts: UInt64, failed: UInt64)
    /// The run stopped on a failure; start a new one after this long.
    case retry(afterMs: UInt64)
    /// The refresh token was refused: log in again. Unsent changes are kept.
    case signedOut
    /// This device was revoked: its cache is cleared and the session locked.
    case deviceRevoked
}

/// Something the app may want to show.
public enum SessionEvent: Sendable, Equatable {
    /// The server refused this item ("couldn't sync"); it stays queued.
    case itemFailed(vaultId: String, itemId: String, status: UInt16)
}

/// One signed-in device's vault: the app's only way into the core (D41 §3).
/// Every server conversation is the core's step loop (D28, D31) run over
/// `HTTPTransport`; the Secret Key, tokens and device id go to `KeyStore`.
/// Never logs values.
public actor Session {
    let client: PwmClient
    private let server: URL
    private let keys: any KeyStore
    private let transport: any HTTPTransport
    /// For the breach check only: straight to HIBP, never through our server.
    private let breachTransport: any HTTPTransport
    /// Holds the biometric unlock key's protection (D45).
    private let biometrics: any BiometricKeyStore
    /// The suggestions above the keyboard (D47 §5), when this app offers them.
    private let suggestions: Suggestions?
    let now: @Sendable () -> Int64
    private let eventsContinuation: AsyncStream<SessionEvent>.Continuation

    /// Item failures and other notices, for the app's UI.
    public nonisolated let events: AsyncStream<SessionEvent>

    /// - Parameters:
    ///   - cachePath: the cache file (see `CacheLocation`), or nil for memory only.
    ///   - server: the sync server's base URL.
    ///   - breachTransport: sends the breach check's range requests (D22 §4).
    ///   - suggestions: kept current after every vault change (D47 §5).
    public init(
        cachePath: String?,
        server: URL,
        keys: any KeyStore,
        transport: any HTTPTransport = URLSessionTransport(),
        breachTransport: any HTTPTransport = URLSessionTransport(),
        biometrics: any BiometricKeyStore = Biometrics.platform,
        suggestions: Suggestions? = nil,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) throws {
        client = try core {
            if let cachePath { try PwmClient.open(path: cachePath) } else { try PwmClient.inMemory() }
        }
        self.server = server
        self.keys = keys
        self.transport = transport
        self.breachTransport = breachTransport
        self.biometrics = biometrics
        self.suggestions = suggestions
        self.now = now
        let (stream, continuation) = AsyncStream.makeStream(of: SessionEvent.self)
        events = stream
        eventsContinuation = continuation
    }

    // MARK: Account

    /// Creates the account and its first vault. Returns the Secret Key to
    /// show on the Emergency Kit now: it is not shown again (it is kept in
    /// `KeyStore` for unlocking).
    public func signUp(email: String, password: Redacted<String>, deviceName: String) async throws -> Redacted<String> {
        let deviceId = try self.deviceId()
        let first = try core {
            try client.startSignup(
                email: email, password: password.value, deviceId: deviceId,
                platform: Session.platform, deviceName: deviceName
            )
        }
        guard case let .signedUp(secretKey) = try await run(first) else {
            throw PWMError(code: "protocol")
        }
        try keys.set(.secretKey, secretKey)
        try keys.set(.email, email)
        try passwordTyped()
        _ = try await run(try core { try client.startEnsureVault() })
        return Redacted(secretKey)
    }

    /// Signs in on this device with all three secrets, then syncs. The core
    /// sends `deviceName`, sealed, once the keys are open (D43 §6).
    public func logIn(
        email: String, password: Redacted<String>, secretKey: Redacted<String>, deviceName: String
    ) async throws -> SyncResult {
        let deviceId = try self.deviceId()
        let first = try core {
            try client.startLogin(
                email: email, password: password.value, secretKey: secretKey.value,
                deviceId: deviceId, platform: Session.platform, deviceName: deviceName
            )
        }
        guard case .loggedIn = try await run(first) else { throw PWMError(code: "protocol") }
        try keys.set(.secretKey, secretKey.value)
        try keys.set(.email, email)
        // A new sign-in starts with Face ID off (D45 §4).
        try disableBiometricUnlock()
        try passwordTyped()
        return try await sync()
    }

    /// Unlocks the account cached on this device, offline.
    public func unlock(password: Redacted<String>) throws {
        guard let secretKey = try keys.get(.secretKey),
              let access = try keys.get(.accessToken),
              let refresh = try keys.get(.refreshToken)
        else { throw PWMError(code: "not_signed_in") }
        try core { try client.unlock(password: password.value, secretKey: secretKey, access: access, refresh: refresh) }
        try passwordTyped()
        // The blob is made again, so it always matches the keyset (D45 §4).
        // After the move to the shared places (D47 §2) only the blob is
        // left, and this makes the key again in the shared group.
        if try keys.get(.biometricBlob) != nil {
            try storeBiometricUnlock(try makeBiometricUnlock())
        }
        refreshSuggestions()
    }

    /// Drops every key from memory; a running conversation is abandoned.
    public func lock() {
        client.lock()
    }

    public var isUnlocked: Bool { client.isUnlocked() }

    /// Whether an account is signed in on this device (it may be locked).
    public func isSignedIn() throws -> Bool { try keys.get(.secretKey) != nil }

    /// Ends this device's session on the server (D62), then deletes the
    /// account from this device: its cache, tokens, Secret Key and email.
    /// The device id stays. Unsent changes are lost. The server gets at most
    /// `signOutTimeout` and any failure is ignored, so being offline never
    /// blocks signing out.
    public func signOut() async throws {
        await endSession()
        try core { try client.signOut() }
        suggestions?.clear()
        try disableBiometricUnlock()
        for name in [KeyName.secretKey, .accessToken, .refreshToken, .email, .passwordUnlockedAt] {
            try keys.delete(name)
        }
    }

    /// How long signing out waits for the server (D62), as for the push token on Android (D60 §4).
    static let signOutTimeout: TimeInterval = 5

    /// `POST /v1/auth/logout` with the stored tokens, locked or not; a
    /// revoked device's cache is gone, so it sends nothing.
    private func endSession() async {
        guard let access = try? keys.get(.accessToken),
              let refresh = try? keys.get(.refreshToken),
              let first = try? core({ try client.startSignOut(access: access, refresh: refresh) })
        else { return }
        let via = transport.withTimeout(Session.signOutTimeout)
        let ending = Task { _ = try? await self.run(first, via: via) }
        let timer = Task {
            try? await Task.sleep(nanoseconds: UInt64(Session.signOutTimeout * 1_000_000_000))
            ending.cancel()
        }
        // A request in flight ends within the transport's timeout; the run then sees the cancel.
        await ending.value
        timer.cancel()
        client.cancel()
    }

    // MARK: Face ID / Touch ID (D45)

    public func biometricState() throws -> BiometricState {
        switch biometrics.availability() {
        case let .unavailable(reason): return .unavailable(reason: reason)
        case let .available(kind): return try keys.get(.biometricBlob) == nil ? .off(kind) : .on(kind)
        }
    }

    /// Turns biometric unlock on while unlocked. It asks for Face ID once,
    /// so a face that doesn't work is found now.
    public func enableBiometricUnlock(reason: String) async throws {
        guard case .available = biometrics.availability() else { throw PWMError(code: "biometric_unavailable") }
        let made = try makeBiometricUnlock()
        do {
            _ = try await biometrics.unwrap(made.wrapped, reason: reason)
        } catch {
            try? biometrics.delete()
            throw error
        }
        try storeBiometricUnlock(made)
        if try keys.get(.passwordUnlockedAt) == nil { try passwordTyped() }
    }

    /// Deletes the Secure Enclave key, the wrapped key and the blob.
    public func disableBiometricUnlock() throws {
        try biometrics.delete()
        try keys.delete(.biometricBlob)
        try keys.delete(.biometricKey)
    }

    /// Unlocks after Face ID. `master_password_due` every 14 days;
    /// `biometric_changed` turns it off, and the master password unlocks.
    public func unlockWithBiometrics(reason: String) async throws {
        guard case .on = try biometricState() else { throw PWMError(code: "biometric_unavailable") }
        let last = try keys.get(.passwordUnlockedAt).flatMap { Int64($0) } ?? 0
        guard now() - last < Biometrics.masterPasswordEveryMs else { throw PWMError(code: "master_password_due") }
        // Moved to the shared places without its key (D47 §2): the next
        // master password unlock makes the key again.
        guard try keys.get(.biometricKey) != nil else { throw PWMError(code: "master_password_due") }
        guard let blob = try keys.get(.biometricBlob).flatMap({ Data(base64Encoded: $0) }),
              let wrapped = try keys.get(.biometricKey).flatMap({ Data(base64Encoded: $0) }),
              let access = try keys.get(.accessToken),
              let refresh = try keys.get(.refreshToken)
        else {
            try disableBiometricUnlock()
            throw PWMError(code: "biometric_changed")
        }
        let key: Redacted<Data>
        do {
            key = try await biometrics.unwrap(wrapped, reason: reason)
        } catch let error as PWMError where error.code == "biometric_changed" {
            try disableBiometricUnlock()
            throw error
        }
        let deviceId = try deviceId()
        do {
            try core {
                try client.unlockBiometric(blob: blob, key: key.value, deviceId: deviceId, access: access, refresh: refresh)
            }
        } catch let error as PWMError where error.code == "biometric_unlock_failed" {
            try disableBiometricUnlock()
            throw PWMError(code: "biometric_changed")
        }
        refreshSuggestions()
    }

    private struct MadeUnlock {
        let blob: Data
        let wrapped: Data
    }

    private func makeBiometricUnlock() throws -> MadeUnlock {
        let deviceId = try deviceId()
        let unlock = try core { try client.exportBiometricUnlock(deviceId: deviceId) }
        let wrapped = try biometrics.wrap(Redacted(unlock.key))
        return MadeUnlock(blob: unlock.blob, wrapped: wrapped)
    }

    private func storeBiometricUnlock(_ made: MadeUnlock) throws {
        try keys.set(.biometricBlob, made.blob.base64EncodedString())
        try keys.set(.biometricKey, made.wrapped.base64EncodedString())
    }

    private func passwordTyped() throws {
        try keys.set(.passwordUnlockedAt, String(now()))
    }

    /// The Emergency Kit PDF for the signed-in account, dated today.
    public func emergencyKit(paper: EmergencyKit.Paper = .of(.current)) throws -> Redacted<Data> {
        guard let email = try keys.get(.email),
              let secretKey = try keys.get(.secretKey),
              let accountId = try core({ try client.accountId() })
        else { throw PWMError(code: "not_signed_in") }
        let created = Date(timeIntervalSince1970: TimeInterval(now()) / 1000)
        return try EmergencyKit.pdf(
            email: email, secretKey: secretKey, server: server, accountId: accountId,
            created: created, paper: paper
        )
    }

    /// The email this device signed in with, for the unlock screen.
    public func email() throws -> String? { try keys.get(.email) }

    // MARK: Sync

    public func sync() async throws -> SyncResult {
        let outcome: Outcome
        do {
            outcome = try await run(try core { try client.startSync() })
        } catch RunEnd.wait(let delayMs) {
            return .retry(afterMs: delayMs)
        }
        return try syncResult(outcome)
    }

    /// A sync for background refresh and pushes (D50 §1-2): it needs only
    /// the tokens, so it runs while locked too. Locked, it stores what needs
    /// no key and uploads what is queued; the rest waits for the next
    /// unlocked sync. Unlocked, it is `sync()`.
    public func backgroundSync() async throws -> SyncResult {
        guard let access = try keys.get(.accessToken), let refresh = try keys.get(.refreshToken) else {
            return .signedOut
        }
        let outcome: Outcome
        do {
            outcome = try await run(try core { try client.startLockedSync(access: access, refresh: refresh) })
        } catch RunEnd.wait(let delayMs) {
            return .retry(afterMs: delayMs)
        }
        return try syncResult(outcome)
    }

    private func syncResult(_ outcome: Outcome) throws -> SyncResult {
        switch outcome {
        case let .synced(pulled, pushed, conflicts, failed, _):
            if pulled > 0 || pushed > 0 { refreshSuggestions() }
            return .synced(pulled: pulled, pushed: pushed, conflicts: conflicts, failed: failed)
        case .signedOut: return .signedOut
        case .deviceRevoked: return .deviceRevoked
        default: throw PWMError(code: "protocol")
        }
    }

    /// This device's APNs token on the server, or none (D50 §3). It needs
    /// only the tokens, so it works while locked.
    public func setPushToken(_ token: Data?, sandbox: Bool) async throws {
        guard let access = try keys.get(.accessToken), let refresh = try keys.get(.refreshToken) else { return }
        let hex = token.map { $0.map { String(format: "%02x", $0) }.joined() }
        let app = Session.platform == .macos ? "macos" : "ios"
        let first = try core {
            try client.startPushToken(access: access, refresh: refresh, app: app, token: hex, sandbox: sandbox)
        }
        do {
            guard case .pushTokenSet = try await run(first) else { throw PWMError(code: "protocol") }
        } catch RunEnd.wait {
            throw PWMError(code: "network")
        }
    }

    /// After the server refused this device's refresh token (D50 §5): the
    /// master password only, since the email and Secret Key are here. The
    /// cache and its unsent changes stay, and so does Face ID; it counts as
    /// a master password unlock. Then syncs.
    public func signInAgain(password: Redacted<String>) async throws -> SyncResult {
        guard let email = try keys.get(.email), let secretKey = try keys.get(.secretKey) else {
            throw PWMError(code: "signed_out")
        }
        let deviceId = try self.deviceId()
        let first = try core {
            try client.startLogin(
                email: email, password: password.value, secretKey: secretKey,
                deviceId: deviceId, platform: Session.platform, deviceName: ""
            )
        }
        guard case .loggedIn = try await run(first) else { throw PWMError(code: "protocol") }
        try passwordTyped()
        return try await sync()
    }

    // MARK: Items

    public func vaultIds() throws -> [String] {
        try core { try client.vaultIds() }
    }

    /// Every item of a vault, trashed ones included (filter on `trashedAtMs`).
    public func items(in vaultId: String) throws -> [Item] {
        try core { try client.items(vaultId: vaultId) }.map(Item.init(json:))
    }

    public func item(_ itemId: String, in vaultId: String) throws -> Item? {
        try core { try client.item(vaultId: vaultId, itemId: itemId) }.map(Item.init(json:))
    }

    /// A new item with a fresh id; save it with `save`.
    public func newItem(_ kind: Item.Kind, title: String) throws -> Item {
        Item(kind: kind, title: title, id: try core { try newId() }, nowMs: now())
    }

    /// Saves locally (history and timestamps are the core's); sync uploads it.
    @discardableResult
    public func save(_ item: Item, in vaultId: String) throws -> String {
        guard item.isEditable else { throw PWMError(code: "read_only_item") }
        let json = try item.text()
        let id = try core { try client.saveItem(vaultId: vaultId, json: json, nowMs: now()) }
        refreshSuggestions()
        return id
    }

    public func setTrashed(_ itemId: String, in vaultId: String, trashed: Bool) throws {
        try core { try client.setTrashed(vaultId: vaultId, itemId: itemId, trashed: trashed, nowMs: now()) }
        refreshSuggestions()
    }

    public func delete(_ itemId: String, in vaultId: String) throws {
        try core { try client.deleteItem(vaultId: vaultId, itemId: itemId) }
        refreshSuggestions()
    }

    /// Deletes items that have been in the trash for 30 days (D19).
    @discardableResult
    public func purgeTrash() throws -> Int {
        Int(try core { try client.purgeTrash(nowMs: now()) })
    }

    // MARK: Suggestions above the keyboard (D47 §5)

    /// "Suggest logins above the keyboard"; on by default.
    public func suggestionsOn() -> Bool { suggestions?.isOn ?? false }

    public func setSuggestions(on: Bool) {
        suggestions?.isOn = on
        refreshSuggestions()
    }

    /// Sends the current list (domains and usernames only) to the system,
    /// or empties it when the switch is off. Needs the vault unlocked; a
    /// failure leaves the last list until the next change.
    func refreshSuggestions() {
        guard let suggestions else { return }
        guard suggestions.isOn else {
            suggestions.clear()
            return
        }
        guard client.isUnlocked(),
              let identities = try? core({ try client.credentialIdentities() })
        else { return }
        suggestions.update(identities.compactMap(Suggestion.init))
    }

    /// The login's current one-time code and the seconds it stays valid.
    public func currentCode(_ itemId: String, in vaultId: String) throws -> (code: Redacted<String>, secondsLeft: UInt64)? {
        try core { try client.currentCode(vaultId: vaultId, itemId: itemId, nowMs: now()) }
            .map { (code: Redacted($0.code), secondsLeft: $0.secondsLeft) }
    }

    // MARK: Devices (D43 §6)

    /// The account's devices, this one marked `isCurrent`.
    public func devices() async throws -> [DeviceEntry] {
        guard case let .devices(devices) = try await runOnline(try core { try client.startListDevices() }) else {
            throw PWMError(code: "protocol")
        }
        return devices.map(DeviceEntry.init)
    }

    /// Renames one of the account's devices (63 bytes at most).
    public func renameDevice(_ deviceId: String, to name: String) async throws {
        try await expectUpdated(try core { try client.startRenameDevice(deviceId: deviceId, name: name) })
    }

    /// Revokes another device; to leave this one, sign out.
    public func revokeDevice(_ deviceId: String) async throws {
        try await expectUpdated(try core { try client.startRevokeDevice(deviceId: deviceId) })
    }

    private func expectUpdated(_ first: Step) async throws {
        guard case .deviceUpdated = try await runOnline(first) else { throw PWMError(code: "protocol") }
    }

    /// A device conversation: a failed run is `network`, and this device
    /// being revoked meanwhile is `device_revoked`.
    private func runOnline(_ first: Step) async throws -> Outcome {
        let outcome: Outcome
        do {
            outcome = try await run(first)
        } catch RunEnd.wait {
            throw PWMError(code: "network")
        }
        switch outcome {
        case .deviceRevoked: throw PWMError(code: "device_revoked")
        case .signedOut: throw PWMError(code: "signed_out")
        default: return outcome
        }
    }

    // MARK: Approving a new device (D46)

    /// The new device's side: asks the account of `email`, calls `onCode`
    /// with the code to compare on the signed-in device, and returns the
    /// Secret Key it sent. Log in with it and the master password. Throws
    /// `approval_ended` when it was denied or expired; cancelling the task
    /// gives up on the server too.
    public func askForApproval(
        email: String, onCode: @escaping @Sendable (String) async -> Void
    ) async throws -> Redacted<String> {
        let ask = try core { try ApprovalAsk(email: email, platform: Session.platform) }
        guard var request = ask.firstRequest() else { throw PWMError(code: "internal") }
        while true {
            let response: HTTPResponse
            do {
                response = try await transport.send(try httpRequest(request))
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                throw PWMError(code: "network")
            }
            let answer = Response(
                status: response.status,
                headers: response.headers.map { Header(name: $0.name, value: $0.value) },
                body: response.body
            )
            switch try core({ try ask.receive(response: answer) }) {
            case let .send(next):
                request = next
                continue
            case let .showCode(code):
                await onCode(code)
                fallthrough
            case .wait:
                do {
                    try await Task.sleep(for: .milliseconds(2000))
                } catch {
                    if let cancel = ask.cancel() { _ = try? await transport.send(try httpRequest(cancel)) }
                    throw CancellationError()
                }
                request = try core { try ask.poll() }
            case let .approved(secretKey):
                return Redacted(secretKey)
            case .ended:
                throw PWMError(code: "approval_ended")
            }
        }
    }

    /// The signed-in side: the account's open requests.
    public func approvalRequests() async throws -> [ApprovalEntry] {
        guard case let .approvals(list) = try await runOnline(try core { try client.startListApprovals() }) else {
            throw PWMError(code: "protocol")
        }
        return list.map {
            ApprovalEntry(
                id: $0.id, platform: $0.platform,
                created: Date(timeIntervalSince1970: TimeInterval($0.createdAt) / 1000),
                state: $0.state, code: $0.code
            )
        }
    }

    /// Accepts a listed request; list again until it has a code.
    public func acceptApproval(_ id: String) async throws {
        try await expectApproval(try core { try client.startAcceptApproval(approvalId: id) })
    }

    /// After the user saw the same code on both devices: sends this device's
    /// Secret Key, sealed for the new one.
    public func completeApproval(_ id: String) async throws {
        guard let secretKey = try keys.get(.secretKey) else { throw PWMError(code: "not_signed_in") }
        try await expectApproval(try core { try client.startCompleteApproval(approvalId: id, secretKey: secretKey) })
    }

    public func denyApproval(_ id: String) async throws {
        try await expectApproval(try core { try client.startDenyApproval(approvalId: id) })
    }

    private func expectApproval(_ first: Step) async throws {
        guard case .approvalUpdated = try await runOnline(first) else { throw PWMError(code: "protocol") }
    }

    // MARK: Plan and billing (D6)

    /// The plan, its subscription, what this server sells and the account's
    /// billing token for an App Store purchase.
    public func billing() async throws -> BillingStatus {
        try await expectBilling(try core { try client.startBillingStatus() })
    }

    /// A Stripe Checkout page for `plan`, to open in the browser (the Mac
    /// app, D6 §2). The plan shows once Stripe tells the server it was paid.
    public func checkoutURL(for plan: Plan) async throws -> URL {
        guard case let .checkoutUrl(url) = try await runBilling(try core { try client.startCheckout(plan: plan.name) }),
              let link = URL(string: url)
        else { throw PWMError(code: "protocol") }
        return link
    }

    /// Cancels the Stripe subscription at the end of its period; one in a
    /// store is `managed_elsewhere`.
    public func cancelSubscription() async throws -> BillingStatus {
        try await expectBilling(try core { try client.startCancelSubscription() })
    }

    /// Lets a canceling Stripe subscription renew again.
    public func resumeSubscription() async throws -> BillingStatus {
        try await expectBilling(try core { try client.startResumeSubscription() })
    }

    /// Refunds the unused part of the last charge and ends the plan now,
    /// for a Stripe or Google Play subscription (D72 §1). After its 30 days,
    /// `refund_window_passed`; an App Store one is `managed_elsewhere`.
    public func refund(provider: BillingProvider) async throws -> BillingStatus {
        try await expectBilling(try core { try client.startRefund(provider: provider.name) })
    }

    /// Hands a finished App Store transaction to the server, which reads it
    /// from Apple (D6 §3). Finish the transaction after this returns.
    public func applePurchase(transactionId: String) async throws -> BillingStatus {
        try await expectBilling(try core { try client.startApplePurchase(transactionId: transactionId) })
    }

    private func expectBilling(_ first: Step) async throws -> BillingStatus {
        guard case let .billing(info) = try await runBilling(first) else { throw PWMError(code: "protocol") }
        return BillingStatus(info)
    }

    /// A billing call: the server or a provider unavailable (a 503, or no
    /// connection) is `billing_unavailable`, to try again later.
    private func runBilling(_ first: Step) async throws -> Outcome {
        let outcome: Outcome
        do {
            outcome = try await run(first)
        } catch RunEnd.wait {
            throw PWMError(code: "billing_unavailable")
        }
        switch outcome {
        case .deviceRevoked: throw PWMError(code: "device_revoked")
        case .signedOut: throw PWMError(code: "signed_out")
        default: return outcome
        }
    }

    // MARK: Health (D22, D43 §7)

    /// Weak and reused passwords across every vault; local only.
    public func passwordHealth() throws -> PasswordHealth {
        let report = try core { try client.healthReport() }
        return PasswordHealth(
            weak: Dictionary(report.weak.map { ($0.itemId, Int($0.score)) }, uniquingKeysWith: { first, _ in first }),
            reused: report.reused
        )
    }

    /// Logins whose password is in known breaches, with how often it was
    /// seen. Only the first 5 characters of each password's SHA-1 leave the
    /// device, straight to HIBP with padding; each prefix is asked once.
    public func breachedPasswords() async throws -> [String: UInt64] {
        let check = try core { try client.breachCheck() }
        let header = breachPaddingHeader()
        for prefix in check.prefixes() {
            guard let url = URL(string: breachRangeUrl(prefix: prefix)) else { throw PWMError(code: "internal") }
            let request = HTTPRequest(method: "GET", url: url, headers: [(name: header.name, value: header.value)], body: nil)
            let response: HTTPResponse
            do {
                response = try await breachTransport.send(request)
            } catch {
                throw PWMError(code: "network")
            }
            guard response.status == 200 else { throw PWMError(code: "breach_check") }
            let body = String(decoding: response.body, as: UTF8.self)
            try core { try check.addResponse(prefix: prefix, body: body) }
        }
        return Dictionary(check.finish().map { ($0.itemId, $0.count) }, uniquingKeysWith: { first, _ in first })
    }

    // MARK: Export (D24, D43 §8)

    /// The master password again; the next `export` within 5 minutes may
    /// run. Face ID never counts.
    public func authorizeExport(password: Redacted<String>) throws {
        guard let secretKey = try keys.get(.secretKey) else { throw PWMError(code: "not_signed_in") }
        try core { try client.authorizeExport(password: password.value, secretKey: secretKey, nowMs: now()) }
    }

    /// Every vault as a file, on every plan. Keep the bytes only until saved.
    public func export(_ format: ExportFile.Format) throws -> Redacted<Data> {
        let coreFormat: PWMCore.ExportFormat = switch format {
        case let .pwmx(password): .pwmx(password: password.value)
        case .json: .json
        case .csv: .csv
        }
        return Redacted(Data(try core { try client.export(format: coreFormat, nowMs: now()) }))
    }

    // MARK: The step loop

    private enum RunEnd: Error {
        case wait(delayMs: UInt64)
    }

    /// Sends each request the core asks for until it is done. A transport
    /// failure goes back to the core as `networkFailed`; cancelling the task
    /// cancels the conversation.
    private func run(_ first: Step, via: (any HTTPTransport)? = nil) async throws -> Outcome {
        var step = first
        while true {
            defer { drainEvents() }
            switch step {
            case let .done(outcome):
                return outcome
            case let .wait(delayMs):
                throw RunEnd.wait(delayMs: delayMs)
            case let .send(request):
                let outgoing = try httpRequest(request)
                let response: HTTPResponse
                do {
                    response = try await (via ?? transport).send(outgoing)
                    try Task.checkCancellation()
                } catch is CancellationError {
                    client.cancel()
                    throw CancellationError()
                } catch {
                    if Task.isCancelled {
                        client.cancel()
                        throw CancellationError()
                    }
                    step = try core { try client.networkFailed() }
                    continue
                }
                let answer = Response(
                    status: response.status,
                    headers: response.headers.map { Header(name: $0.name, value: $0.value) },
                    body: response.body
                )
                step = try core { try client.receive(response: answer, nowMs: now()) }
            }
        }
    }

    private func httpRequest(_ request: Request) throws -> HTTPRequest {
        let base = server.absoluteString.hasSuffix("/") ? String(server.absoluteString.dropLast()) : server.absoluteString
        guard let url = URL(string: base + request.path) else { throw PWMError(code: "invalid_url") }
        return HTTPRequest(
            method: request.method,
            url: url,
            headers: request.headers.map { (name: $0.name, value: $0.value) },
            body: request.body
        )
    }

    /// New tokens go to `KeyStore`; the rest to `events`.
    private func drainEvents() {
        for event in client.takeEvents() {
            switch event {
            case let .tokens(access, refresh):
                // A failure here leaves the old tokens; the next refresh retries.
                try? keys.set(.accessToken, access)
                try? keys.set(.refreshToken, refresh)
            case let .itemFailed(vaultId, itemId, status):
                eventsContinuation.yield(.itemFailed(vaultId: vaultId, itemId: itemId, status: status))
            }
        }
    }

    private func deviceId() throws -> String {
        if let id = try keys.get(.deviceId) { return id }
        let id = try core { try newId() }
        try keys.set(.deviceId, id)
        return id
    }

    static var platform: Platform {
        #if os(iOS)
        .ios
        #elseif os(macOS)
        .macos
        #else
        .linux
        #endif
    }
}
