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
    private let client: PwmClient
    private let server: URL
    private let keys: any KeyStore
    private let transport: any HTTPTransport
    /// For the breach check only: straight to HIBP, never through our server.
    private let breachTransport: any HTTPTransport
    private let now: @Sendable () -> Int64
    private let eventsContinuation: AsyncStream<SessionEvent>.Continuation

    /// Item failures and other notices, for the app's UI.
    public nonisolated let events: AsyncStream<SessionEvent>

    /// - Parameters:
    ///   - cachePath: the cache file (see `CacheLocation`), or nil for memory only.
    ///   - server: the sync server's base URL.
    ///   - breachTransport: sends the breach check's range requests (D22 §4).
    public init(
        cachePath: String?,
        server: URL,
        keys: any KeyStore,
        transport: any HTTPTransport = URLSessionTransport(),
        breachTransport: any HTTPTransport = URLSessionTransport(),
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) throws {
        client = try core {
            if let cachePath { try PwmClient.open(path: cachePath) } else { try PwmClient.inMemory() }
        }
        self.server = server
        self.keys = keys
        self.transport = transport
        self.breachTransport = breachTransport
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
        return try await sync()
    }

    /// Unlocks the account cached on this device, offline.
    public func unlock(password: Redacted<String>) throws {
        guard let secretKey = try keys.get(.secretKey),
              let access = try keys.get(.accessToken),
              let refresh = try keys.get(.refreshToken)
        else { throw PWMError(code: "not_signed_in") }
        try core { try client.unlock(password: password.value, secretKey: secretKey, access: access, refresh: refresh) }
    }

    /// Drops every key from memory; a running conversation is abandoned.
    public func lock() {
        client.lock()
    }

    public var isUnlocked: Bool { client.isUnlocked() }

    /// Whether an account is signed in on this device (it may be locked).
    public func isSignedIn() throws -> Bool { try keys.get(.secretKey) != nil }

    /// Deletes the account from this device: its cache, tokens, Secret Key
    /// and email. The device id stays. Unsent changes are lost.
    public func signOut() throws {
        try core { try client.signOut() }
        for name in [KeyName.secretKey, .accessToken, .refreshToken, .email] {
            try keys.delete(name)
        }
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
        switch outcome {
        case let .synced(pulled, pushed, conflicts, failed, _):
            return .synced(pulled: pulled, pushed: pushed, conflicts: conflicts, failed: failed)
        case .signedOut: return .signedOut
        case .deviceRevoked: return .deviceRevoked
        default: throw PWMError(code: "protocol")
        }
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
        return try core { try client.saveItem(vaultId: vaultId, json: json, nowMs: now()) }
    }

    public func setTrashed(_ itemId: String, in vaultId: String, trashed: Bool) throws {
        try core { try client.setTrashed(vaultId: vaultId, itemId: itemId, trashed: trashed, nowMs: now()) }
    }

    public func delete(_ itemId: String, in vaultId: String) throws {
        try core { try client.deleteItem(vaultId: vaultId, itemId: itemId) }
    }

    /// Deletes items that have been in the trash for 30 days (D19).
    @discardableResult
    public func purgeTrash() throws -> Int {
        Int(try core { try client.purgeTrash(nowMs: now()) })
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
    private func run(_ first: Step) async throws -> Outcome {
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
                    response = try await transport.send(outgoing)
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
