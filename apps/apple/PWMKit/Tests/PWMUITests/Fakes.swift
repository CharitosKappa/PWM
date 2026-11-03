import Foundation
import XCTest
@testable import PWMKit
@testable import PWMUI

/// An account with no server: records what the screens ask for and keeps
/// items in memory, as the core's cache would.
actor FakeAccount: Account {
    var signedIn = false
    var storedEmail: String?
    var calls: [String] = []
    var unlockPassword = "correct horse battery staple 42"
    var failure: PWMError?
    var vaults = ["vault-1"]
    var stored: [String: [String: Item]] = ["vault-1": [:]]
    var syncResult: SyncResult = .synced(pulled: 0, pushed: 0, conflicts: 0, failed: 0)
    var code: (code: Redacted<String>, secondsLeft: UInt64)?
    var codeFails = false
    var nextId = 0
    let nowMs: Int64 = 1_767_225_600_000

    func setSignedIn(email: String) {
        signedIn = true
        storedEmail = email
    }

    func fail(with code: String?) {
        failure = code.map(PWMError.init(code:))
    }

    func setSyncResult(_ result: SyncResult) {
        syncResult = result
    }

    func setCode(_ value: String?, secondsLeft: UInt64 = 20, fails: Bool = false) {
        code = value.map { (code: Redacted($0), secondsLeft: secondsLeft) }
        codeFails = fails
    }

    func put(_ item: Item, in vault: String = "vault-1") {
        stored[vault, default: [:]][item.id!] = item
    }

    func storedItem(_ id: String, in vault: String = "vault-1") -> Item? {
        stored[vault]?[id]
    }

    private func check() throws {
        if let failure { throw failure }
    }

    func isSignedIn() -> Bool { signedIn }
    func email() -> String? { storedEmail }

    func signUp(email: String, password: Redacted<String>, deviceName: String) throws -> Redacted<String> {
        try check()
        calls.append("signUp \(email) \(deviceName)")
        setSignedIn(email: email)
        return Redacted("A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA")
    }

    func logIn(email: String, password: Redacted<String>, secretKey: Redacted<String>, deviceName: String) throws -> SyncResult {
        try check()
        calls.append("logIn \(email) \(password.value) \(secretKey.value) \(deviceName)")
        setSignedIn(email: email)
        return .synced(pulled: 0, pushed: 0, conflicts: 0, failed: 0)
    }

    func unlock(password: Redacted<String>) throws {
        guard password.value == unlockPassword else { throw PWMError(code: "wrong_password") }
        calls.append("unlock")
    }

    func lock() { calls.append("lock") }

    func signOut() {
        calls.append("signOut")
        signedIn = false
        storedEmail = nil
        stored = [:]
    }

    func emergencyKit(paper: EmergencyKit.Paper) throws -> Redacted<Data> {
        try check()
        calls.append("kit \(paper)")
        return Redacted(Data("%PDF-fake".utf8))
    }

    func sync() throws -> SyncResult {
        try check()
        calls.append("sync")
        return syncResult
    }

    func vaultIds() -> [String] { vaults }

    func items(in vaultId: String) throws -> [Item] {
        try check()
        return Array((stored[vaultId] ?? [:]).values)
    }

    func newItem(_ kind: Item.Kind, title: String) -> Item {
        nextId += 1
        return Item(kind: kind, title: title, id: "new-\(nextId)", nowMs: nowMs)
    }

    func save(_ item: Item, in vaultId: String) throws -> String {
        try check()
        guard item.isEditable else { throw PWMError(code: "read_only_item") }
        calls.append("save \(item.type)")
        put(item, in: vaultId)
        return item.id!
    }

    func setTrashed(_ itemId: String, in vaultId: String, trashed: Bool) throws {
        try check()
        guard let item = stored[vaultId]?[itemId] else { throw PWMError(code: "unknown_item") }
        var json = item.json
        json["trashed_at"] = trashed ? .int(nowMs) : .null
        put(try Item(json: try JSONValue.object(json).text()), in: vaultId)
        calls.append(trashed ? "trash" : "restore")
    }

    func delete(_ itemId: String, in vaultId: String) throws {
        try check()
        stored[vaultId]?[itemId] = nil
        calls.append("delete")
    }

    func purgeTrash() -> Int {
        calls.append("purge")
        return 0
    }

    func currentCode(_ itemId: String, in vaultId: String) throws -> (code: Redacted<String>, secondsLeft: UInt64)? {
        if codeFails { throw PWMError(code: "invalid_item") }
        return code
    }

    // Devices, health and export.

    var deviceList: [DeviceEntry] = []
    var health = PasswordHealth(weak: [:], reused: [])
    var breached: [String: UInt64] = [:]
    var breachFailure: PWMError?
    var exportAuthorized = false

    var biometric: BiometricState = .off(.faceID)
    var biometricFailure: PWMError?

    func setBiometric(_ state: BiometricState, failure: String? = nil) {
        biometric = state
        biometricFailure = failure.map(PWMError.init(code:))
    }

    func biometricState() -> BiometricState { biometric }

    func enableBiometricUnlock(reason: String) throws {
        if let biometricFailure, biometricFailure.code == "biometric_cancelled" { throw biometricFailure }
        guard case let .off(kind) = biometric else { throw PWMError(code: "biometric_unavailable") }
        biometric = .on(kind)
        calls.append("enable biometric")
    }

    func disableBiometricUnlock() {
        if case let .on(kind) = biometric { biometric = .off(kind) }
        calls.append("disable biometric")
    }

    func unlockWithBiometrics(reason: String) throws {
        if let biometricFailure {
            if biometricFailure.code == "biometric_changed", case let .on(kind) = biometric { biometric = .off(kind) }
            throw biometricFailure
        }
        guard case .on = biometric else { throw PWMError(code: "biometric_unavailable") }
        calls.append("unlock biometric")
    }

    // Suggestions above the keyboard (D47).
    var suggestions = true

    func suggestionsOn() -> Bool { suggestions }

    func setSuggestions(on: Bool) {
        suggestions = on
        calls.append("suggestions \(on ? "on" : "off")")
    }

    // Approving a new device.
    var approvalList: [ApprovalEntry] = []
    var askCode = "123 456"
    var askResult: Result<String, PWMError> = .success(kitKey)

    func setApprovals(_ list: [ApprovalEntry]) { approvalList = list }
    func setAsk(code: String, result: Result<String, PWMError>) {
        askCode = code
        askResult = result
    }

    func askForApproval(email: String, onCode: @escaping @Sendable (String) async -> Void) async throws -> Redacted<String> {
        calls.append("ask \(email)")
        await onCode(askCode)
        return Redacted(try askResult.get())
    }

    func approvalRequests() throws -> [ApprovalEntry] {
        try check()
        return approvalList
    }

    func acceptApproval(_ id: String) throws {
        calls.append("accept \(id)")
        approvalList = approvalList.map {
            $0.id == id ? ApprovalEntry(id: $0.id, platform: $0.platform, created: $0.created, state: "revealed", code: "654 321") : $0
        }
    }

    func completeApproval(_ id: String) throws {
        calls.append("complete \(id)")
        approvalList.removeAll { $0.id == id }
    }

    func denyApproval(_ id: String) throws {
        calls.append("deny \(id)")
        approvalList.removeAll { $0.id == id }
    }

    func setDevices(_ devices: [DeviceEntry]) { deviceList = devices }
    func setHealth(_ health: PasswordHealth, breached: [String: UInt64] = [:], breachFailure: String? = nil) {
        self.health = health
        self.breached = breached
        self.breachFailure = breachFailure.map(PWMError.init(code:))
    }

    func devices() throws -> [DeviceEntry] {
        try check()
        return deviceList
    }

    func renameDevice(_ deviceId: String, to name: String) throws {
        try check()
        guard let index = deviceList.firstIndex(where: { $0.id == deviceId }) else { throw PWMError(code: "unknown_device") }
        let d = deviceList[index]
        deviceList[index] = DeviceEntry(id: d.id, platform: d.platform, name: name, added: d.added, lastSeen: d.lastSeen, revoked: d.revoked, isCurrent: d.isCurrent)
        calls.append("rename \(deviceId) \(name)")
    }

    func revokeDevice(_ deviceId: String) throws {
        try check()
        guard let index = deviceList.firstIndex(where: { $0.id == deviceId }) else { throw PWMError(code: "unknown_device") }
        let d = deviceList[index]
        deviceList[index] = DeviceEntry(id: d.id, platform: d.platform, name: d.name, added: d.added, lastSeen: d.lastSeen, revoked: Date(), isCurrent: d.isCurrent)
        calls.append("revoke \(deviceId)")
    }

    func passwordHealth() throws -> PasswordHealth {
        try check()
        return health
    }

    func breachedPasswords() throws -> [String: UInt64] {
        calls.append("breaches")
        if let breachFailure { throw breachFailure }
        return breached
    }

    func authorizeExport(password: Redacted<String>) throws {
        guard password.value == unlockPassword else { throw PWMError(code: "wrong_password") }
        exportAuthorized = true
    }

    func export(_ format: ExportFile.Format) throws -> Redacted<Data> {
        guard exportAuthorized else { throw PWMError(code: "export_not_authorized") }
        exportAuthorized = false
        switch format {
        case let .pwmx(password):
            calls.append("export pwmx \(password.value)")
            return Redacted(Data("encrypted".utf8))
        case .json:
            calls.append("export json")
            return Redacted(Data("{}".utf8))
        case .csv:
            calls.append("export csv")
            return Redacted(Data("title".utf8))
        }
    }
}

@MainActor
final class FakePasteboard: SecretPasteboard {
    var copied: [(String, TimeInterval)] = []

    func copy(_ value: Redacted<String>, clearAfter seconds: TimeInterval) {
        copied.append((value.value, seconds))
    }
}

let strong = "correct horse battery staple 42"
let kitKey = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA"

/// A login item for tests.
func loginItem(_ id: String, _ title: String, username: String = "", url: String? = nil, password: String = "") -> Item {
    var item = Item(kind: .login, title: title, id: id, nowMs: 1)
    var login = Login()
    login.username = username
    login.password = password
    if let url { login.urls = [LoginURL(url: url)] }
    item.login = login
    return item
}
